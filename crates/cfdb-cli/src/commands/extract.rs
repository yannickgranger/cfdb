//! Extraction command handlers — the `cfdb extract` entry and the
//! working-tree extract path. Split out of `commands.rs` for the drift
//! god-file decomposition (#151); the `--rev`/URL/cache subsystem moved
//! on to `extract_rev.rs` (#560 decomposition, boy-scouted in #556).

use std::path::{Path, PathBuf};
// RFC-048 §3.1 (slice 48-A): the composition root owns the profiling clock —
// the extractor emits pure phase markers and this crate does every wall-clock
// read. Gated on `lang-rust` because only the Rust profiled path uses it.
#[cfg(feature = "lang-rust")]
use std::time::{Duration, Instant};

use cfdb_core::schema::Keyspace;
use cfdb_core::store::StoreBackend;

use crate::compose;

use super::extract_rev::{extract_at_rev, extract_at_url_rev, is_url_at_sha};

pub fn keyspace_path(db: &Path, keyspace: &str) -> PathBuf {
    db.join(format!("{keyspace}.json"))
}

/// Default keyspace name when `--keyspace` is absent: the workspace
/// directory's basename. Only the working-tree path uses it — the `--rev`
/// paths default to the short rev instead (see `extract_rev`).
fn workspace_basename(workspace: &Path) -> String {
    workspace
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "workspace".to_string())
}

/// Surface ingest-time diagnostics on stderr (RFC-054 §3.4, 54-A #556).
/// Exit stays 0 — the warning is diagnostic, not failure. Runs after every
/// ingest (syn + optional HIR) and before save, so the stderr report matches
/// what the keyspace persists. Lives with the command that owns the
/// `extract:` prefix; one stderr lock for the whole batch.
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
    // The `Some(rev) if is_url_at_sha(rev)` guard is the SINGLE resolution
    // point for URL-vs-SHA discrimination. Do not duplicate this check
    // inside `extract_at_rev` or `extract_at_url_rev` — see the Wiring
    // Assertion in `.prescriptions/96.md`.
    match rev.as_deref() {
        None => extract_at_path(&workspace, &db, keyspace, hir, no_proc_macro, profile),
        Some(rev) if is_url_at_sha(rev) => {
            extract_at_url_rev(rev, &db, keyspace, hir, no_proc_macro, profile)
        }
        Some(rev) => extract_at_rev(&workspace, rev, &db, keyspace, hir, no_proc_macro, profile),
    }
}

/// Extract the current working tree at `workspace` into a keyspace on
/// disk. This is the v0.1 behaviour preserved verbatim — `extract`
/// dispatches here when `--rev` is absent.
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

    // RFC-048 §3.1 (slice 48-A) — the profiled path attributes wall-clock
    // across the extract's phases and prints the breakdown to stderr. It is
    // Rust-specific (the phases are the Rust extract pipeline's, RFC-048 §1),
    // so it runs the Rust extractor directly rather than the polyglot
    // producer registry below. Timings never touch the keyspace, so a
    // profiled extract's graph is byte-identical to a plain one (RFC-048 §4).
    if profile {
        return run_profiled_extract(workspace, db, &ks, &ks_name, hir, no_proc_macro);
    }

    // RFC-041 Phase 1 / Slice 41-C dispatcher — replaces the direct
    // `cfdb_extractor::extract_workspace` call with a producer-trait
    // lookup. The composition root lives in `crate::lang`; this is
    // the only call site that touches the registry. Slim builds
    // (`--no-default-features`) hit the `[]` arm and surface
    // `NoProducerDetected` cleanly.
    let producers = crate::lang::available_producers();
    let compiled_in: Vec<&'static str> = producers.iter().map(|p| p.name()).collect();
    let matched: Vec<&dyn cfdb_lang::LanguageProducer> = producers
        .iter()
        .filter(|p| p.detect(workspace))
        .map(|boxed| boxed.as_ref())
        .collect();

    let (nodes, edges) = match matched.as_slice() {
        [] => {
            // An empty or no-recognized-language workspace is NOT an error: a
            // fact-graph extractor over a repo with no source it understands
            // answers with an EMPTY graph, not a failure. This lets a
            // greenfield bootstrap — a brand-new repo whose first commit will
            // create the workspace — be x-rayed into a valid, empty keyspace
            // that downstream consumers (anchor resolution, violation checks)
            // read as "zero items" instead of crashing on a missing keyspace.
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

/// Composition-root clock for RFC-048 profiling (slice 48-A). The extractor
/// emits pure [`cfdb_extractor::ExtractPhaseMarker`] boundaries — it reads no
/// clock (RFC-029 §12.1 G1) — and this observer timestamps each. All
/// wall-clock reads for the extract-internal phases live here, never in the
/// extractor.
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
    /// Record the wall-clock instant at which `marker` fired.
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

    /// The three extract-internal phase durations `(cargo-metadata, syn-walk,
    /// deferred-resolve)` derived from the four boundary marks, or `None` if
    /// any mark is missing. The extractor fires all four in order every run,
    /// so `None` means the profiled contract broke — the caller surfaces that
    /// as an error rather than reporting a fabricated zero.
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

/// RFC-048 §3.1 (slice 48-A) profiled extract. Runs the Rust extractor's
/// profiled entry point with a [`PhaseClock`] observer that times the three
/// `extract`-internal phases (cargo-metadata, syn-walk, deferred-resolve),
/// times the CLI-owned ingest / `--hir` load / save phases around their
/// existing calls, and renders the [`crate::ExtractProfile`] breakdown to
/// stderr. The keyspace bytes are identical to a non-profiled extract — every
/// clock read is here (RFC-029 §12.1 G1 keeps them out of the extractor) and
/// none enters the graph (RFC-048 §4).
///
/// Rust-only: the profiled phase vocabulary is the Rust pipeline's, so this
/// runs `cfdb_extractor` directly instead of the polyglot producer registry.
/// Compiled only under the `lang-rust` feature; the slim build hits the stub
/// below and reports the missing feature instead of silently no-op'ing.
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

    // The composition root owns the clock: the extractor emits pure phase
    // markers and this observer timestamps each. `PhaseClock` then derives the
    // three extract-internal phase durations from the four marks. The `&mut`
    // borrow of `clock` ends when the call returns, freeing it for the
    // duration read below.
    let mut clock = PhaseClock::default();
    // `ExtractError` maps into `CfdbCliError::Extract` via `#[from]` (both are
    // `lang-rust`-gated), so `?` surfaces the same typed failure the
    // non-profiled path's `ExtractError` consumers see.
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

/// Slim-build stub — `--profile` is a Rust-pipeline diagnostic (RFC-048 §1),
/// so a build with no `lang-rust` feature cannot honour it. Mirrors the
/// [`extract_hir`] `#[cfg(not(...))]` stub: a clear error, never a silent
/// no-op.
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

/// Run the HIR pipeline when the `hir` feature is compiled in. The
/// feature-gated `crate::hir::extract_and_ingest_hir` module is the
/// single integration seam — if not compiled, `--hir` emits a clear
/// error instead of silently no-oping.
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
