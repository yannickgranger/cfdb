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
use cfdb_petgraph::{persist, PetgraphStore};

use crate::commands::keyspace_path;
use crate::CfdbCliError;

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
/// slice 43-B; future `enrich_rfc_docs`, `enrich_concepts`). `None` ⇒
/// identical behaviour to [`load_store`]. Separate function (not a default
/// arg on `load_store`) so the 20+ existing call sites stay signature-stable
/// — clean-arch B4 resolution from `council/43/clean-arch.md`.
pub(crate) fn load_store_with_workspace(
    db: &Path,
    keyspace: &str,
    workspace_root: Option<PathBuf>,
) -> Result<(PetgraphStore, Keyspace), CfdbCliError> {
    let ks = Keyspace::new(keyspace);
    let path = keyspace_path(db, keyspace);
    let mut store = match workspace_root {
        Some(root) => PetgraphStore::new().with_workspace(root),
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
