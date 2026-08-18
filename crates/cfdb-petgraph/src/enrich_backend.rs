//! `EnrichBackend` implementation for `PetgraphStore`.
//!
//! Enrichment is a sibling trait. PetgraphStore inherits the
//! seven stubs (`EnrichReport::not_implemented`); only
//! `enrich_deprecation` is overridden here.
//!
//! `enrich_deprecation` overridden to report the real
//! source as the extractor rather than deflecting to `not_implemented`. The
//! deprecation facts (`is_deprecated`, `deprecation_since`) are populated at
//! extraction time by `cfdb-extractor` via `extract_deprecated_attr`, so the
//! `EnrichBackend::enrich_deprecation` method is a runtime no-op but must
//! advertise its non-stub status — `ran: true, attrs_written: 0` with a
//! warning naming the extractor so callers can distinguish "done upstream"
//! from "deferred".

use cfdb_core::enrich::EnrichBackend;
use cfdb_core::store::StoreError;

use crate::PetgraphStore;

impl PetgraphStore {
    /// Guard #1 — keyspace existence. Returns `Err(UnknownKeyspace)` if the
    /// caller's target keyspace is not known to the store; otherwise `Ok(())`.
    fn require_keyspace(&self, keyspace: &cfdb_core::schema::Keyspace) -> Result<(), StoreError> {
        if !self.keyspaces.contains_key(keyspace) {
            return Err(StoreError::UnknownKeyspace(keyspace.clone()));
        }
        Ok(())
    }
}

impl EnrichBackend for PetgraphStore {
    fn enrich_deprecation(
        &mut self,
        keyspace: &cfdb_core::schema::Keyspace,
    ) -> Result<cfdb_core::enrich::EnrichReport, StoreError> {
        self.require_keyspace(keyspace)?;
        Ok(cfdb_core::enrich::EnrichReport {
            verb: "enrich_deprecation".into(),
            ran: true,
            facts_scanned: 0,
            attrs_written: 0,
            edges_written: 0,
            warnings: vec![
                "enrich_deprecation: facts populated at extraction time by cfdb-extractor::extract_deprecated_attr (#43-C / RFC addendum §A2.2 row 3); no enrichment work to do"
                    .into(),
            ],
        })
    }
}

// ---------------------------------------------------------------------------
// These tests pin the exact report fields and warning text below. A
// failure means behavior changed — whether the old behavior was correct is
// a separate question these tests do not answer.
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use cfdb_core::enrich::EnrichBackend;
    use cfdb_core::schema::Keyspace;
    use cfdb_core::store::StoreBackend;

    use crate::PetgraphStore;

    fn store_with_empty_keyspace(ks: &Keyspace) -> PetgraphStore {
        let mut store = PetgraphStore::new();
        store.ingest_nodes(ks, vec![]).expect("register keyspace");
        store
    }

    #[test]
    fn deprecation_pins_fixed_report_shape() {
        let ks = Keyspace::new("test");
        let mut store = store_with_empty_keyspace(&ks);

        let report = store.enrich_deprecation(&ks).expect("pass");

        assert!(report.ran);
        assert_eq!(report.facts_scanned, 0);
        assert_eq!(report.attrs_written, 0);
        assert_eq!(report.edges_written, 0);
        assert_eq!(
            report.warnings,
            vec!["enrich_deprecation: facts populated at extraction time by \
                 cfdb-extractor::extract_deprecated_attr (#43-C / RFC \
                 addendum §A2.2 row 3); no enrichment work to do"
                .to_string()]
        );
    }

    #[test]
    fn deprecation_unknown_keyspace_returns_err() {
        let mut store = PetgraphStore::new();
        let ks = Keyspace::new("never");

        let err = store
            .enrich_deprecation(&ks)
            .expect_err("unknown keyspace must err");

        assert!(format!("{err:?}").contains("UnknownKeyspace"));
    }
}
