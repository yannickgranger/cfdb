//! Auxiliary command handlers — `cfdb dump`, `cfdb list-keyspaces`, and
//! the `cfdb export` alias. Split out of `commands.rs` for the drift
//! god-file decomposition (#151). Move-only; public paths preserved via
//! `pub use` in `commands.rs`.

use std::path::PathBuf;

use cfdb_core::store::StoreBackend;

use crate::compose;

pub fn dump(db: PathBuf, keyspace: String) -> Result<(), crate::CfdbCliError> {
    let (store, ks) = compose::load_store(&db, &keyspace)?;
    let dump = store.canonical_dump(&ks)?;
    println!("{dump}");
    Ok(())
}

pub fn list_keyspaces(db: PathBuf) -> Result<(), crate::CfdbCliError> {
    if !db.exists() {
        return Ok(());
    }
    let names = compose::list_keyspace_names(&db)?;
    for n in names {
        println!("{n}");
    }
    Ok(())
}

/// `cfdb export` — alias of `cfdb dump` with a `--format` flag for forward
/// compatibility. v0.1 only supports `sorted-jsonl` (the canonical dump).
pub fn export(db: PathBuf, keyspace: String, format: &str) -> Result<(), crate::CfdbCliError> {
    if format != "sorted-jsonl" {
        return Err(format!("unsupported --format `{format}`. v0.1 supports: sorted-jsonl").into());
    }
    dump(db, keyspace)
}
