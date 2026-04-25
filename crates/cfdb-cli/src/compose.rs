//! Composition root for `cfdb-cli` (RFC-031 §4).
//!
//! This module is the **single place in cfdb-cli that knows which concrete
//! `StoreBackend` is wired in**. Every other handler module constructs and
//! loads its store exclusively through `compose::*` — `commands.rs`,
//! `scope.rs`, `stubs.rs`, and `enrich.rs` never call `PetgraphStore::new()`
//! or `persist::{load, save}` directly.
//!
//! # Why a single composition root
//!
//! Before this module existed, `PetgraphStore::new()` was instantiated
//! directly in four handler modules. A decision to swap the backend (say, to
//! a future `LadybugStore` or to a test double) required editing every
//! handler. The SOLID audit (issue #23, RFC-031 §4) flagged this as a Clean
//! Architecture + SRP violation: handler modules owned both command semantics
//! AND infrastructure construction.
//!
//! With this module in place, swapping the backend is a one-file change.
//! Handler modules depend on the factory functions here, not on the concrete
//! `PetgraphStore` type.
//!
//! # Why these functions return a concrete type
//!
//! The factories return `PetgraphStore` by value (not `Box<dyn StoreBackend>`)
//! because cfdb-cli's handlers currently need specific petgraph-side
//! operations (e.g. `persist::save` takes `&PetgraphStore`). The concrete
//! return keeps the adapter seam honest — when a second backend exists, the
//! factory signature becomes the negotiation point (feature flag, env var,
//! config), and the adapter abstraction lands naturally at that time.
//!
//! # Scope boundary
//!
//! This module does NOT own:
//! - the keyspace-path layout (`keyspace_path` lives in `commands.rs`);
//! - command semantics (each handler module owns its own dispatch);
//! - persistence formats (`cfdb-petgraph::persist` owns that).

use std::path::{Path, PathBuf};

use cfdb_core::schema::Keyspace;
use cfdb_petgraph::index::spec::IndexSpec;
use cfdb_petgraph::{persist, PetgraphStore};

use crate::commands::keyspace_path;
use crate::CfdbCliError;

/// Relative path inside `workspace_root` for the optional index spec.
/// Missing file is not an error (see [`IndexSpec::from_path`]).
const INDEXES_TOML_PATH: &str = ".cfdb/indexes.toml";

/// Construct an empty in-memory store. Used by the extract path before ingest.
pub(crate) fn empty_store() -> PetgraphStore {
    PetgraphStore::new()
}

/// Load a keyspace from the on-disk database into a fresh `PetgraphStore`.
///
/// Returns the loaded store plus the `Keyspace` handle callers need for
/// subsequent backend calls. The caller is free to validate path existence
/// before calling this factory if it wants to emit a command-specific error
/// message — the factory itself propagates whatever `persist::load` returns
/// via the typed error.
pub(crate) fn load_store(
    db: &Path,
    keyspace: &str,
) -> Result<(PetgraphStore, Keyspace), CfdbCliError> {
    load_store_with_workspace(db, keyspace, None)
}

/// Variant that also attaches a workspace root to the store. Used by
/// enrichment verbs that read workspace files (`enrich_git_history` —
/// slice 43-B; future `enrich_rfc_docs`, `enrich_concepts`) and by the
/// slice-7 `cfdb scope` path that needs `.cfdb/indexes.toml` wired.
/// `None` ⇒ identical behaviour to [`load_store`]. Separate function
/// (not a default arg on `load_store`) so the 20+ existing call sites
/// stay signature-stable — clean-arch B4 resolution from
/// `council/43/clean-arch.md`.
///
/// When `workspace_root = Some(root)`, this is the single composition
/// root for `.cfdb/indexes.toml` per RFC-035 §3.8 — no other code path
/// reads the TOML. Missing file → `IndexSpec::empty()`, not an error.
pub(crate) fn load_store_with_workspace(
    db: &Path,
    keyspace: &str,
    workspace_root: Option<PathBuf>,
) -> Result<(PetgraphStore, Keyspace), CfdbCliError> {
    let ks = Keyspace::new(keyspace);
    let path = keyspace_path(db, keyspace);
    let mut store = match workspace_root {
        Some(root) => {
            let spec = IndexSpec::from_path(&root.join(INDEXES_TOML_PATH))
                .map_err(|e| CfdbCliError::from(format!("load .cfdb/indexes.toml: {e}")))?;
            PetgraphStore::new().with_workspace(root).with_indexes(spec)
        }
        None => empty_store(),
    };
    persist::load(&mut store, &ks, &path)?;
    Ok((store, ks))
}

/// Persist a keyspace to the on-disk database. Creates the `db` directory if
/// missing. Returns the path the keyspace was written to so callers can print
/// a locate-my-file message without re-computing it.
pub(crate) fn save_store(
    store: &PetgraphStore,
    keyspace: &Keyspace,
    db: &Path,
) -> Result<PathBuf, CfdbCliError> {
    std::fs::create_dir_all(db)?;
    let path = keyspace_path(db, keyspace.as_str());
    persist::save(store, keyspace, &path)?;
    Ok(path)
}

/// Returns the sorted list of keyspace names found under `<db>/`.
///
/// A keyspace name is the file_stem of a `.json` direct child. Returns an
/// empty `Vec` if the directory is missing or contains no `.json` files —
/// missing directory is not an error here, callers that want a "db missing"
/// error message must check `db.exists()` themselves before invoking.
///
/// Centralised in the composition root so the read_dir / extension-filter /
/// file_stem / sort recipe lives once (audit 2026-W17, EPIC #273, Pattern 3
/// finding cfdb-cli F-011). Callers requiring count-based semantics
/// (`resolve_keyspace_name`'s 0/1/N branch) layer their own logic on top of
/// the returned `Vec<String>`.
pub(crate) fn list_keyspace_names(db: &Path) -> Result<Vec<String>, CfdbCliError> {
    if !db.exists() {
        return Ok(Vec::new());
    }
    let mut names: Vec<String> = std::fs::read_dir(db)?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) == Some("json") {
                p.file_stem().and_then(|s| s.to_str()).map(String::from)
            } else {
                None
            }
        })
        .collect();
    names.sort();
    Ok(names)
}

#[cfg(test)]
mod tests {
    //! Slice-7 (#186) — `load_store_with_workspace` reads
    //! `.cfdb/indexes.toml` at the composition root. Missing file is
    //! not an error (returns `IndexSpec::empty()`); invalid TOML is an
    //! error; valid TOML populates the store's `IndexSpec`.

    use cfdb_core::schema::Keyspace;
    use cfdb_core::store::StoreBackend;

    use super::*;

    const SAMPLE_INDEXES_TOML: &str = r#"
[[index]]
label = "Item"
prop = "qname"
notes = "slice-7 compose test"

[[index]]
label = "Item"
computed = "last_segment(qname)"
notes = "slice-7 compose test"
"#;

    fn seed_db(db: &Path, keyspace: &str) {
        // Build an empty-keyspace JSON file so `persist::load` has
        // something to consume. The keyspace file format is a
        // `KeyspaceFile { schema_version, nodes, edges }` — empty
        // vectors for nodes/edges yield a valid roundtrippable dump.
        std::fs::create_dir_all(db).expect("mkdir db");
        let ks = Keyspace::new(keyspace);
        let mut store = PetgraphStore::new();
        // Auto-create the keyspace via ingest of an empty batch so
        // `save_store`/`export` find it.
        StoreBackend::ingest_nodes(&mut store, &ks, Vec::new()).expect("seed ingest");
        save_store(&store, &ks, db).expect("seed save_store");
    }

    #[test]
    fn load_store_with_workspace_none_has_empty_spec() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let db = tmp.path().join("db");
        seed_db(&db, "ks0");
        let (store, _ks) = load_store_with_workspace(&db, "ks0", None).expect("load");
        assert!(
            store.workspace_root().is_none(),
            "workspace=None must leave workspace_root unset"
        );
        assert!(
            store.index_spec().is_empty(),
            "workspace=None must yield IndexSpec::empty()"
        );
    }

    #[test]
    fn load_store_with_workspace_some_missing_toml_is_empty_spec() {
        // Missing `.cfdb/indexes.toml` must NOT be an error — RFC §3.8.
        let tmp = tempfile::tempdir().expect("tempdir");
        let db = tmp.path().join("db");
        let ws = tmp.path().join("ws");
        std::fs::create_dir_all(&ws).expect("mkdir ws");
        seed_db(&db, "ks1");
        let (store, _ks) =
            load_store_with_workspace(&db, "ks1", Some(ws)).expect("load with missing toml");
        assert!(
            store.workspace_root().is_some(),
            "workspace_root must be set even when indexes.toml is missing"
        );
        assert!(
            store.index_spec().is_empty(),
            "missing .cfdb/indexes.toml must yield IndexSpec::empty(), not error"
        );
    }

    #[test]
    fn load_store_with_workspace_some_reads_indexes_toml() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let db = tmp.path().join("db");
        let ws = tmp.path().join("ws");
        std::fs::create_dir_all(ws.join(".cfdb")).expect("mkdir .cfdb");
        std::fs::write(ws.join(".cfdb/indexes.toml"), SAMPLE_INDEXES_TOML).expect("write toml");
        seed_db(&db, "ks2");

        let (store, _ks) = load_store_with_workspace(&db, "ks2", Some(ws)).expect("load with toml");
        assert_eq!(
            store.index_spec().entries.len(),
            2,
            "IndexSpec must contain the two [[index]] entries from the sample TOML"
        );
    }

    #[test]
    fn load_store_with_workspace_invalid_toml_errors() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let db = tmp.path().join("db");
        let ws = tmp.path().join("ws");
        std::fs::create_dir_all(ws.join(".cfdb")).expect("mkdir .cfdb");
        std::fs::write(ws.join(".cfdb/indexes.toml"), "this is not valid toml = [").expect("write");
        seed_db(&db, "ks3");
        let result = load_store_with_workspace(&db, "ks3", Some(ws));
        assert!(
            result.is_err(),
            "malformed .cfdb/indexes.toml must propagate as CfdbCliError"
        );
    }
}
