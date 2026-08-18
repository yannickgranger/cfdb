//! `ClassifyEngine` — the judgment layer's entry point, generic over the
//! store like its siblings `EnrichEngine` and `QueryEngine`.
//!
//! The engine is dispatch and orchestration only: it holds a `QueryEngine`
//! (the one way it reaches a keyspace), validates the requested context, runs
//! the classifier rules through the `scope` primitives and assembles the
//! typed payloads. It never loads a store, never writes output, never exits —
//! the composition root does all of that. Rule execution and Cypher
//! construction stay in the `scope` submodules.

use std::collections::BTreeSet;

use cfdb_core::graph::GraphBackend;
use cfdb_core::schema::Keyspace;
use cfdb_core::store::StoreError;
use cfdb_eval::QueryEngine;
use cfdb_query::DiffEnvelope;
use thiserror::Error;

use crate::classify::{collect_restrict_qnames, ClassifyEnvelope, DiffSourceMeta};
use crate::explain::ExplainSink;
use crate::scope::{
    attach_scope_warnings, build_scope_inventory, populate_findings_by_class_restricted,
    validate_context,
};
use crate::taxonomy::ScopeInventory;

/// Knobs a caller can turn on a `scope` run. Every knob defaults to off.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScopeOptions {
    /// Run the production-only `Unwired` rule (`reachable_from_production_entry`)
    /// instead of the all-kinds default. `cfdb scope --production-only`;
    /// `cfdb classify` never sets it.
    pub production_only: bool,
}

/// Everything a `scope` / `classify` run can fail with. `Store` and `Parse`
/// wrap the upstream errors verbatim; `UnknownContext` renders the exact
/// message the CLI has always printed.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ClassifyError {
    /// A store-level failure on a query the engine cannot degrade
    /// (the inventory query, `hsb-by-name`, the `:Context` listing).
    #[error(transparent)]
    Store(#[from] StoreError),
    /// An embedded rule failed to parse — a build defect, never a user error.
    #[error("parse error in embedded {rule}: {source}")]
    Parse {
        /// Which embedded text failed (`classifier rule`, `hsb-by-name template`).
        rule: &'static str,
        #[source]
        source: cfdb_query::ParseError,
    },
    /// The requested bounded context is not a `:Context` node of the keyspace.
    #[error("unknown context `{context}`; known contexts: [{}]", known.join(", "))]
    UnknownContext {
        context: String,
        /// Every `:Context.name` in the keyspace, sorted, so the caller can
        /// print the fix.
        known: Vec<String>,
    },
}

/// The judgment engine over one store. Reaches the graph only through
/// [`QueryEngine`] (`GraphBackend` port); holds no path, no file, no
/// process state.
pub struct ClassifyEngine<'s, S: GraphBackend> {
    query: QueryEngine<'s, S>,
}

impl<'s, S: GraphBackend> ClassifyEngine<'s, S> {
    /// Build the engine over `store`. Cheap; holds `&'s S`.
    pub fn new(store: &'s S) -> Self {
        Self {
            query: QueryEngine::new(store),
        }
    }

    /// The underlying query engine, for callers that need to run their own
    /// Cypher on the same store (the CLI's `--explain` printing shares it).
    pub fn query(&self) -> &QueryEngine<'s, S> {
        &self.query
    }

    /// `cfdb scope` — the structured infection inventory for one bounded
    /// context: every class bucket filled by its classifier rule, canonical
    /// candidates from `hsb-by-name`, per-crate item counts, and the
    /// degradation warnings inside the inventory (`ScopeInventory::warnings`).
    /// `explain`, when given, collects one trace row per query it runs.
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

    /// `cfdb classify` — the same classification restricted to the items a
    /// `DiffEnvelope` added or changed, wrapped in the versioned
    /// [`ClassifyEnvelope`]. Always the all-kinds `Unwired` rule.
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
}
