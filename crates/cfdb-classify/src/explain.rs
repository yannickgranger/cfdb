//! Thin interior-mutability wrapper so every scope helper can accept a
//! shared `&ExplainSink` argument without threading
//! `&mut Option<Vec<ExplainRow>>` through five layers.
//!
//! When disabled, every method is a no-op and no allocation happens beyond
//! the zero-sized wrapper. When enabled, each query execution pushes its
//! collected [`ExplainRow`]s into the shared `Vec`, which the caller drains
//! once all queries have run.

use std::cell::RefCell;

use cfdb_core::graph::GraphBackend;
use cfdb_core::query::Query;
use cfdb_core::result::QueryResult;
use cfdb_core::schema::Keyspace;
use cfdb_core::store::{QueryBackend, StoreError};
use cfdb_eval::explain::ExplainRow;
use cfdb_eval::QueryEngine;

/// The `--explain` accumulator. `None` inside the cell means "disabled";
/// `Some(vec)` means "collecting".
pub struct ExplainSink {
    inner: RefCell<Option<Vec<ExplainRow>>>,
}

impl ExplainSink {
    /// A collecting sink: every query run through it appends its trace rows.
    pub fn enabled() -> Self {
        Self {
            inner: RefCell::new(Some(Vec::new())),
        }
    }

    /// A no-op sink: queries take the plain `execute` path.
    pub fn disabled() -> Self {
        Self {
            inner: RefCell::new(None),
        }
    }

    /// Whether this sink collects.
    pub fn is_enabled(&self) -> bool {
        self.inner.borrow().is_some()
    }

    /// Run `query` on `engine`, routing through `execute_explained` when
    /// the sink is enabled so the trace rows flow back into `self`.
    /// When disabled, falls through to the plain `execute` path with
    /// zero overhead.
    pub(crate) fn run<S: GraphBackend>(
        &self,
        engine: &QueryEngine<'_, S>,
        ks: &Keyspace,
        query: &Query,
    ) -> Result<QueryResult, StoreError> {
        if self.is_enabled() {
            let (result, rows) = engine.execute_explained(ks, query)?;
            if let Some(buf) = self.inner.borrow_mut().as_mut() {
                buf.extend(rows);
            }
            Ok(result)
        } else {
            engine.execute(ks, query)
        }
    }

    /// Drain the collected rows. Leaves the sink in the disabled
    /// state — the trace is consumed once.
    pub fn drain(&self) -> Vec<ExplainRow> {
        self.inner.borrow_mut().take().unwrap_or_default()
    }
}
