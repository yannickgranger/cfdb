use std::path::PathBuf;

use cfdb_core::enrich::{EnrichBackend, EnrichReport};

use crate::compose;
use crate::output;

pub enum EnrichVerb {
    GitHistory,
    RfcDocs,
    Deprecation,
    BoundedContext,
    Concepts,
    Reachability,
    Metrics,
}

pub fn enrich(
    db: PathBuf,
    keyspace: String,
    verb: EnrichVerb,
    workspace: Option<PathBuf>,
) -> Result<(), crate::CfdbCliError> {
    let (mut store, ks) = compose::load_store_with_workspace(&db, &keyspace, workspace)?;

    let report: EnrichReport = match verb {
        EnrichVerb::GitHistory => {
            cfdb_enrich::EnrichEngine::new(&mut store).enrich_git_history(&ks)?
        }
        EnrichVerb::RfcDocs => cfdb_enrich::EnrichEngine::new(&mut store).enrich_rfc_docs(&ks)?,
        EnrichVerb::Deprecation => {
            cfdb_enrich::EnrichEngine::new(&mut store).enrich_deprecation(&ks)?
        }
        EnrichVerb::BoundedContext => {
            cfdb_enrich::EnrichEngine::new(&mut store).enrich_bounded_context(&ks)?
        }
        EnrichVerb::Concepts => cfdb_enrich::EnrichEngine::new(&mut store).enrich_concepts(&ks)?,
        EnrichVerb::Reachability => {
            cfdb_enrich::EnrichEngine::new(&mut store).enrich_reachability(&ks)?
        }
        EnrichVerb::Metrics => cfdb_enrich::EnrichEngine::new(&mut store).enrich_metrics(&ks)?,
    };

    if report.ran && (report.attrs_written > 0 || report.edges_written > 0) {
        compose::save_store(&store, &ks, &db)?;
    }

    output::emit_json(&report)
}
