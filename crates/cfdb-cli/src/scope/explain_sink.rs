//! Thin interior-mutability wrapper so every `cfdb scope` helper can
//! accept a shared `&ExplainSink` argument without threading
//! `&mut Option<Vec<ExplainRow>>` through five layers.
//!
//! Slice-7 (#186) — activated by `cfdb scope --explain`. When
//! disabled, every method is a no-op and no allocation happens beyond
//! the zero-sized wrapper. When enabled, each query execution pushes
//! its collected [`ExplainRow`]s into the shared `Vec`, which the
//! caller drains and prints to stderr once all queries have run.

use std::cell::RefCell;

use cfdb_core::query::Query;
use cfdb_core::result::QueryResult;
use cfdb_core::schema::Keyspace;
use cfdb_core::store::{StoreBackend, StoreError};
use cfdb_petgraph::explain::ExplainRow;
use cfdb_petgraph::PetgraphStore;

/// Encapsulates the `--explain` accumulator. `None` inside the cell
/// means "disabled"; `Some(vec)` means "collecting".
pub(super) struct ExplainSink {
    inner: RefCell<Option<Vec<ExplainRow>>>,
}

impl ExplainSink {
    pub(super) fn enabled() -> Self {
        Self {
            inner: RefCell::new(Some(Vec::new())),
        }
    }

    pub(super) fn disabled() -> Self {
        Self {
            inner: RefCell::new(None),
        }
    }

    pub(super) fn is_enabled(&self) -> bool {
        self.inner.borrow().is_some()
    }

    /// Run `query` on `store`, routing through `execute_explained` when
    /// the sink is enabled so the trace rows flow back into `self`.
    /// When disabled, falls through to the plain `execute` path with
    /// zero overhead.
    pub(super) fn run(
        &self,
        store: &PetgraphStore,
        ks: &Keyspace,
        query: &Query,
    ) -> Result<QueryResult, StoreError> {
        if self.is_enabled() {
            let (result, rows) = store.execute_explained(ks, query)?;
            if let Some(buf) = self.inner.borrow_mut().as_mut() {
                buf.extend(rows);
            }
            Ok(result)
        } else {
            store.execute(ks, query)
        }
    }

    /// Drain the collected rows. Leaves the sink in the disabled
    /// state — the CLI consumes the trace once and exits.
    pub(super) fn drain(&self) -> Vec<ExplainRow> {
        self.inner.borrow_mut().take().unwrap_or_default()
    }
}
