//! Auxiliary command handlers — `cfdb dump`, `cfdb list-keyspaces`, and
//! the `cfdb export` alias. Split out of `commands.rs` for the drift
//! god-file decomposition (#151). Move-only; public paths preserved via
//! `pub use` in `commands.rs`.

use std::path::PathBuf;
use std::str::FromStr;

use cfdb_core::store::StoreBackend;

use crate::compose;
use crate::output::OutputFormat;

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
///
/// EPIC #273 Pattern 1 #4: parse via the canonical `OutputFormat`, then
/// narrow to the per-handler allowlist via `require_one_of`. The "v0.1
/// supports: sorted-jsonl" wording is dropped in favour of the unified
/// "expected `<wire>`" shape — there is no integration test asserting on
/// the v0.1 phrasing for `export`, and the unified message is more
/// consistent with the other four sites this verb shares its parser with.
pub fn export(db: PathBuf, keyspace: String, format: &str) -> Result<(), crate::CfdbCliError> {
    let _format =
        OutputFormat::from_str(format)?.require_one_of(&[OutputFormat::SortedJsonl], "export")?;
    dump(db, keyspace)
}
