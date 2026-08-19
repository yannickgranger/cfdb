use std::path::{Path, PathBuf};
#[cfg(feature = "lang-rust")]
use std::time::{Duration, Instant};

use cfdb_core::schema::Keyspace;
use cfdb_core::store::StoreBackend;

use crate::compose;

use super::extract_rev::{extract_at_rev, extract_at_url_rev, is_url_at_sha};

pub fn keyspace_path(db: &Path, keyspace: &str) -> PathBuf {
    db.join(format!("{keyspace}.json"))
}

fn workspace_basename(workspace: &Path) -> String {
    workspace
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "workspace".to_string())
}

fn surface_ingest_warnings(store: &cfdb_petgraph::PetgraphStore, ks: &Keyspace) {
    use std::io::Write;
    let warnings = store.ingest_warnings(ks);
    if warnings.is_empty() {
        return;
    }
    let stderr = std::io::stderr();
    let mut err = stderr.lock();
    let _ = writeln!(err, "extract: {} ingest warning(s)", warnings.len());
    for w in &warnings {
        let _ = writeln!(err, "extract: warning: {}", w.message);
        if let Some(s) = &w.suggestion {
            let _ = writeln!(err, "extract:   suggestion: {s}");
        }
    }
}

pub fn extract(
    workspace: PathBuf,
    db: PathBuf,
    keyspace: Option<String>,
    hir: bool,
    no_proc_macro: bool,
    rev: Option<String>,
    profile: bool,
) -> Result<(), crate::CfdbCliError> {
    match rev.as_deref() {
        None => extract_at_path(&workspace, &db, keyspace, hir, no_proc_macro, profile),
        Some(rev) if is_url_at_sha(rev) => {
            extract_at_url_rev(rev, &db, keyspace, hir, no_proc_macro, profile)
        }
        Some(rev) => extract_at_rev(&workspace, rev, &db, keyspace, hir, no_proc_macro, profile),
    }
}

pub(super) fn extract_at_path(
    workspace: &Path,
    db: &Path,
    keyspace: Option<String>,
    hir: bool,
    no_proc_macro: bool,
    profile: bool,
) -> Result<(), crate::CfdbCliError> {
    let ks_name = keyspace.unwrap_or_else(|| workspace_basename(workspace));
    let ks = Keyspace::new(&ks_name);

    eprintln!("extract: walking {}", workspace.display());

    if profile {
        return run_profiled_extract(workspace, db, &ks, &ks_name, hir, no_proc_macro);
    }

    let producers = crate::lang::available_producers();
    let compiled_in: Vec<&'static str> = producers.iter().map(|p| p.name()).collect();
    let matched: Vec<&dyn cfdb_lang::LanguageProducer> = producers
        .iter()
        .filter(|p| p.detect(workspace))
        .map(|boxed| boxed.as_ref())
        .collect();

    let (nodes, edges) = match matched.as_slice() {
        [] => {
            eprintln!(
                "cfdb: no LanguageProducer detected workspace `{}`; \
                 compiled-in producers: {compiled_in:?} — extracting an empty graph",
                workspace.display()
            );
            (Vec::new(), Vec::new())
        }
        [single] => single.produce(workspace)?,
        [first, rest @ ..] => {
            let other_names: Vec<&'static str> = rest.iter().map(|p| p.name()).collect();
            eprintln!(
                "cfdb: polyglot workspace; v0.1 dispatch picks `{}` (also detected: {:?}). \
                 A future `--lang` flag will let you override.",
                first.name(),
                other_names
            );
            first.produce(workspace)?
        }
    };

    eprintln!("extract: {} nodes, {} edges", nodes.len(), edges.len());

    let mut store = compose::empty_store();
    store.ingest_nodes(&ks, nodes)?;
    store.ingest_edges(&ks, edges)?;

    if hir {
        extract_hir(&mut store, &ks, workspace, !no_proc_macro)?;
    }

    surface_ingest_warnings(&store, &ks);

    let path = compose::save_store(&store, &ks, db)?;
    eprintln!("extract: saved keyspace `{ks_name}` to {}", path.display());
    Ok(())
}

#[cfg(feature = "lang-rust")]
#[derive(Default)]
struct PhaseClock {
    cargo_metadata_start: Option<Instant>,
    syn_walk_start: Option<Instant>,
    deferred_resolve_start: Option<Instant>,
    finished: Option<Instant>,
}

#[cfg(feature = "lang-rust")]
impl PhaseClock {
    fn observe(&mut self, marker: cfdb_extractor::ExtractPhaseMarker) {
        use cfdb_extractor::ExtractPhaseMarker as Marker;

        let now = Instant::now();
        match marker {
            Marker::CargoMetadataStart => self.cargo_metadata_start = Some(now),
            Marker::SynWalkStart => self.syn_walk_start = Some(now),
            Marker::DeferredResolveStart => self.deferred_resolve_start = Some(now),
            Marker::Finished => self.finished = Some(now),
        }
    }

    fn phase_durations(&self) -> Option<(Duration, Duration, Duration)> {
        let cargo_metadata_start = self.cargo_metadata_start?;
        let syn_walk_start = self.syn_walk_start?;
        let deferred_resolve_start = self.deferred_resolve_start?;
        let finished = self.finished?;
        Some((
            syn_walk_start.saturating_duration_since(cargo_metadata_start),
            deferred_resolve_start.saturating_duration_since(syn_walk_start),
            finished.saturating_duration_since(deferred_resolve_start),
        ))
    }
}

#[cfg(feature = "lang-rust")]
fn run_profiled_extract(
    workspace: &Path,
    db: &Path,
    ks: &Keyspace,
    ks_name: &str,
    hir: bool,
    no_proc_macro: bool,
) -> Result<(), crate::CfdbCliError> {
    use crate::ExtractProfile;

    let t_total = Instant::now();

    let mut clock = PhaseClock::default();
    let (nodes, edges) =
        cfdb_extractor::extract_workspace_profiled(workspace, &mut |m| clock.observe(m))?;
    eprintln!("extract: {} nodes, {} edges", nodes.len(), edges.len());

    let (cargo_metadata, syn_walk, deferred_resolve) =
        clock.phase_durations().ok_or_else(|| {
            crate::CfdbCliError::from(
                "profiled extract did not emit every phase marker — the observer contract broke"
                    .to_string(),
            )
        })?;

    let mut store = compose::empty_store();
    let t_ingest = Instant::now();
    store.ingest_nodes(ks, nodes)?;
    store.ingest_edges(ks, edges)?;
    let ingest = t_ingest.elapsed();

    let hir_load = if hir {
        let t_hir = Instant::now();
        extract_hir(&mut store, ks, workspace, !no_proc_macro)?;
        Some(t_hir.elapsed())
    } else {
        None
    };

    surface_ingest_warnings(&store, ks);

    let t_save = Instant::now();
    let path = compose::save_store(&store, ks, db)?;
    let save = t_save.elapsed();
    eprintln!("extract: saved keyspace `{ks_name}` to {}", path.display());

    let profile = ExtractProfile {
        cargo_metadata,
        syn_walk,
        deferred_resolve,
        ingest,
        hir_load,
        save,
        total: t_total.elapsed(),
    };
    eprint!("{}", profile.render());
    Ok(())
}

#[cfg(not(feature = "lang-rust"))]
fn run_profiled_extract(
    _workspace: &Path,
    _db: &Path,
    _ks: &Keyspace,
    _ks_name: &str,
    _hir: bool,
    _no_proc_macro: bool,
) -> Result<(), crate::CfdbCliError> {
    Err(crate::CfdbCliError::from(
        "`--profile` requires the `lang-rust` feature — RFC-048 profiles the Rust extract pipeline (rebuild with default features or `--features lang-rust`)".to_string(),
    ))
}

#[cfg(feature = "hir")]
fn extract_hir(
    store: &mut cfdb_petgraph::PetgraphStore,
    ks: &Keyspace,
    workspace: &Path,
    proc_macros: bool,
) -> Result<(), crate::CfdbCliError> {
    crate::hir::extract_and_ingest_hir(store, ks, workspace, proc_macros)
        .map_err(|e| crate::CfdbCliError::from(format!("hir extract failed: {e}")))?;
    Ok(())
}

#[cfg(not(feature = "hir"))]
fn extract_hir(
    _store: &mut cfdb_petgraph::PetgraphStore,
    _ks: &Keyspace,
    _workspace: &Path,
    _proc_macros: bool,
) -> Result<(), crate::CfdbCliError> {
    Err(crate::CfdbCliError::from(
        "`--hir` requires the `hir` Cargo feature — rebuild with `cargo build -p cfdb-cli --features hir`".to_string(),
    ))
}
