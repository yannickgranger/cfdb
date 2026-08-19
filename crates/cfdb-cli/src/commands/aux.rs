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

pub fn export(db: PathBuf, keyspace: String, format: &str) -> Result<(), crate::CfdbCliError> {
    let _format =
        OutputFormat::from_str(format)?.require_one_of(&[OutputFormat::SortedJsonl], "export")?;
    dump(db, keyspace)
}
