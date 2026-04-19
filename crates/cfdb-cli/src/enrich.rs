//! `cfdb enrich-*` verbs.
//!
//! Split out of `lib.rs` for the god-file decomposition (#3751). Public
//! surface preserved: every item here is re-exported from the crate root.

use std::path::PathBuf;

use cfdb_core::enrich::{EnrichBackend, EnrichReport};

use crate::compose;

/// Which `enrich_*` verb to dispatch to. Lets one handler function service all
/// four CLI variants without duplicating the load-store-print boilerplate.
pub enum EnrichVerb {
    Docs,
    Metrics,
    History,
    Concepts,
}

pub fn enrich(db: PathBuf, keyspace: String, verb: EnrichVerb) -> Result<(), crate::CfdbCliError> {
    let (mut store, ks) = compose::load_store(&db, &keyspace)?;

    let report: EnrichReport = match verb {
        EnrichVerb::Docs => store.enrich_docs(&ks)?,
        EnrichVerb::Metrics => store.enrich_metrics(&ks)?,
        EnrichVerb::History => store.enrich_history(&ks)?,
        EnrichVerb::Concepts => store.enrich_concepts(&ks)?,
    };
    let json = serde_json::to_string_pretty(&report)?;
    println!("{json}");
    Ok(())
}
