use std::cell::RefCell;

use cfdb_core::graph::GraphBackend;
use cfdb_core::query::Query;
use cfdb_core::result::QueryResult;
use cfdb_core::schema::Keyspace;
use cfdb_core::store::{QueryBackend, StoreError};
use cfdb_eval::explain::ExplainRow;
use cfdb_eval::QueryEngine;

pub struct ExplainSink {
    inner: RefCell<Option<Vec<ExplainRow>>>,
}

impl ExplainSink {
    pub fn enabled() -> Self {
        Self {
            inner: RefCell::new(Some(Vec::new())),
        }
    }

    pub fn disabled() -> Self {
        Self {
            inner: RefCell::new(None),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.inner.borrow().is_some()
    }

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

    pub fn drain(&self) -> Vec<ExplainRow> {
        self.inner.borrow_mut().take().unwrap_or_default()
    }
}
