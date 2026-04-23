//! Scar — MCP handler with two parse paths both producing `Stop`.
//!
//! Violation of the CLAUDE.md MCP Boundary Fix AC Template: the
//! handler has a handwritten parser (`parse_stop_raw`) alongside the
//! domain's canonical `Stop::from_str`. Both resolve to `Stop`; both
//! reachable from the entry point → VSB fork.
//!
//! `vsb-multi-resolver.cypher` MUST flag this handler:
//!   entry_point   = "stop_request"
//!   param_name    = "stop"
//!   param_type    = "mcp_boundary_fix_ac_scar::Stop"
//!   resolvers     = [Stop::from_str, mcp::parse_stop_raw]

pub enum Stop {
    Fixed,
    Trailing,
    Compound,
}

impl Stop {
    pub fn from_str(raw: &str) -> Option<Self> {
        match raw {
            "fixed" => Some(Stop::Fixed),
            "trailing" => Some(Stop::Trailing),
            "compound" => Some(Stop::Compound),
            _ => None,
        }
    }
}

pub mod mcp {
    use super::Stop;

    /// Handwritten parser that accepts legacy aliases — exists because
    /// some older clients send `"stop-loss"` or `"ts"` and the domain
    /// `FromStr` doesn't. The correct fix is to broaden `FromStr`, but
    /// for now we have two resolvers. VSB detector flags this.
    pub fn parse_stop_raw(raw: &str) -> Option<Stop> {
        match raw {
            "stop-loss" | "fixed" => Some(Stop::Fixed),
            "ts" | "trailing" => Some(Stop::Trailing),
            "cs" | "compound" => Some(Stop::Compound),
            _ => None,
        }
    }

    pub fn handle_stop_request(stop: &str) -> Option<Stop> {
        if stop.contains('-') {
            parse_stop_raw(stop)
        } else {
            Stop::from_str(stop)
        }
    }
}
