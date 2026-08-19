use cfdb_core::graph::GraphBackend;
use cfdb_core::query::Query;
use cfdb_core::result::QueryResult;
use cfdb_core::schema::Keyspace;
use cfdb_core::store::{QueryBackend, StoreError};

use crate::eval::Evaluator;
use crate::explain::ExplainRow;

pub struct QueryEngine<'s, S> {
    store: &'s S,
}

impl<'s, S: GraphBackend> QueryEngine<'s, S> {
    pub fn new(store: &'s S) -> Self {
        Self { store }
    }

    pub fn execute_explained(
        &self,
        keyspace: &Keyspace,
        query: &Query,
    ) -> Result<(QueryResult, Vec<ExplainRow>), StoreError> {
        let reader = self.store.graph_reader(keyspace)?;
        let (mut result, explain) =
            Evaluator::new_with_explain(reader, &query.params).run_explained(query);
        let mut prepended = reader.ingest_warnings();
        prepended.append(&mut result.warnings);
        result.warnings = prepended;
        Ok((result, explain))
    }
}

impl<'s, S: GraphBackend> QueryBackend for QueryEngine<'s, S> {
    fn execute(&self, keyspace: &Keyspace, query: &Query) -> Result<QueryResult, StoreError> {
        let reader = self.store.graph_reader(keyspace)?;
        let mut result = Evaluator::new(reader, &query.params).run(query);
        let mut prepended = reader.ingest_warnings();
        prepended.append(&mut result.warnings);
        result.warnings = prepended;
        Ok(result)
    }
}
