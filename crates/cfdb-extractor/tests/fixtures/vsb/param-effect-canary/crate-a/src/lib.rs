//! Clean baseline — one resolver per registered param.
//!
//! The MCP handler `handle_timeframe_query` registers a `Timeframe`
//! param and delegates to exactly one resolver (`Timeframe::from_str`).
//! `cfdb` should emit:
//!   - :EntryPoint (mcp_tool) → EXPOSES → :Item handle_timeframe_query
//!   - :EntryPoint → REGISTERS_PARAM → :Param { type_normalized: Timeframe }
//!   - :Item handle_timeframe_query → CALLS → :Item Timeframe::from_str
//!   - :Item Timeframe::from_str → RETURNS → :Item Timeframe
//!
//! Since only ONE resolver returns Timeframe, vsb-multi-resolver.cypher
//! must NOT report this entry point.

pub enum Timeframe {
    H1,
    H4,
    D1,
}

impl Timeframe {
    pub fn from_str(raw: &str) -> Option<Self> {
        match raw {
            "1h" => Some(Timeframe::H1),
            "4h" => Some(Timeframe::H4),
            "1d" => Some(Timeframe::D1),
            _ => None,
        }
    }
}

pub fn handle_timeframe_query(timeframe: &str) -> Option<Timeframe> {
    Timeframe::from_str(timeframe)
}
