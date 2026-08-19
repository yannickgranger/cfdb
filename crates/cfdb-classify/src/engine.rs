use std::collections::BTreeSet;

use cfdb_core::graph::GraphBackend;
use cfdb_core::schema::Keyspace;
use cfdb_core::store::StoreError;
use cfdb_eval::QueryEngine;
use cfdb_query::DiffEnvelope;
use thiserror::Error;

use crate::check::{t1, t3, CheckReport, TriggerId};
use crate::classify::{collect_restrict_qnames, ClassifyEnvelope, DiffSourceMeta};
use crate::explain::ExplainSink;
use crate::scope::{
    attach_scope_warnings, build_scope_inventory, populate_findings_by_class_restricted,
    validate_context,
};
use crate::taxonomy::ScopeInventory;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScopeOptions {
    pub production_only: bool,
}

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ClassifyError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error("parse error in embedded {rule}: {source}")]
    Parse {
        rule: &'static str,
        #[source]
        source: cfdb_query::ParseError,
    },
    #[error("unknown context `{context}`; known contexts: [{}]", known.join(", "))]
    UnknownContext { context: String, known: Vec<String> },
}

pub struct ClassifyEngine<'s, S: GraphBackend> {
    query: QueryEngine<'s, S>,
}

impl<'s, S: GraphBackend> ClassifyEngine<'s, S> {
    pub fn new(store: &'s S) -> Self {
        Self {
            query: QueryEngine::new(store),
        }
    }

    pub fn query(&self) -> &QueryEngine<'s, S> {
        &self.query
    }

    pub fn scope(
        &self,
        keyspace: &Keyspace,
        context: &str,
        opts: &ScopeOptions,
        explain: Option<&ExplainSink>,
    ) -> Result<ScopeInventory, ClassifyError> {
        validate_context(&self.query, keyspace, context)?;
        let disabled = ExplainSink::disabled();
        let sink = explain.unwrap_or(&disabled);
        build_scope_inventory(&self.query, keyspace, context, sink, opts.production_only)
    }

    pub fn classify(
        &self,
        keyspace: &Keyspace,
        context: &str,
        diff: &DiffEnvelope,
    ) -> Result<ClassifyEnvelope, ClassifyError> {
        validate_context(&self.query, keyspace, context)?;
        let restrict: BTreeSet<String> = collect_restrict_qnames(diff);
        let diff_source = DiffSourceMeta {
            a: diff.a.clone(),
            b: diff.b.clone(),
            restrict_count: restrict.len() as u64,
        };
        let sink = ExplainSink::disabled();
        let mut inventory = ScopeInventory::new(context, keyspace.as_str());
        populate_findings_by_class_restricted(
            &self.query,
            keyspace,
            context,
            &restrict,
            &mut inventory,
            &sink,
        )?;
        attach_scope_warnings(&mut inventory);
        Ok(ClassifyEnvelope::new(inventory, diff_source))
    }

    pub fn check(
        &self,
        keyspace: &Keyspace,
        trigger: TriggerId,
    ) -> Result<CheckReport, ClassifyError> {
        match trigger {
            TriggerId::T1 => t1::run(&self.query, keyspace),
            TriggerId::T3 => t3::run(&self.query, keyspace),
        }
    }
}
