//! Per-trigger handler modules. Each handler is a pure function that reads
//! argv-supplied TOML + diff inputs and returns a [`TriggerOutcome`].
//!
//! Handlers perform zero I/O beyond `std::fs::read_to_string` on paths they
//! are explicitly given. They never write files, never spawn subprocesses,
//! and never cache (Forbidden move #5: stateless).

pub mod c1_cross_context;
pub mod c3_port_signature;
pub mod c7_financial_precision;
pub mod c8_pipeline_stage;
pub mod c9_workspace_cardinality;

/// Result of evaluating one C-trigger against a diff snapshot.
#[derive(Debug)]
pub struct TriggerOutcome {
    /// Whether the trigger fired (i.e. pre-council review is MANDATED).
    pub fired: bool,
    /// Machine-readable evidence payload embedded in the report envelope
    /// under `evidence.<TRIGGER_ID>`. Empty object when no evidence to show.
    pub evidence: serde_json::Value,
}
