pub mod c1_cross_context;
pub mod c3_port_signature;
pub mod c7_financial_precision;
pub mod c8_pipeline_stage;
pub mod c9_workspace_cardinality;

#[derive(Debug)]
pub struct TriggerOutcome {
    pub fired: bool,
    pub evidence: serde_json::Value,
}
