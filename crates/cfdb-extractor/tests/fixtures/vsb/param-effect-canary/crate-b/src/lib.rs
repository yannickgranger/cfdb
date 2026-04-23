//! Scar — TWO resolvers reachable from the same entry point.
//!
//! The MCP handler registers a `Timeframe` param. The handler body
//! dispatches on the raw string shape: short codes go through
//! `Timeframe::from_str`, provider-formatted (Capital-exchange) codes
//! go through `Timeframe::from_capital`. Both return `Timeframe`, both
//! are reachable from the single entry point — classic VSB fork.
//!
//! `vsb-multi-resolver.cypher` MUST flag this handler:
//!   entry_point   = "timeframe_query"
//!   param_name    = "timeframe"
//!   param_type    = "param_effect_canary_scar::Timeframe"
//!   resolvers     = [Timeframe::from_capital, Timeframe::from_str]

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

    /// Provider-specific parse path. The existence of a second resolver
    /// returning `Timeframe`, reachable from the same entry point, is
    /// the VSB fork the detector fires on.
    pub fn from_capital(raw: &str) -> Option<Self> {
        match raw {
            "HOUR" => Some(Timeframe::H1),
            "FOUR_HOUR" => Some(Timeframe::H4),
            "DAY" => Some(Timeframe::D1),
            _ => None,
        }
    }
}

pub fn handle_timeframe_query(timeframe: &str) -> Option<Timeframe> {
    if timeframe.chars().next().is_some_and(|c| c.is_ascii_uppercase()) {
        Timeframe::from_capital(timeframe)
    } else {
        Timeframe::from_str(timeframe)
    }
}
