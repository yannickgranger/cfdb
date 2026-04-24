//! Clean baseline — handler parses via domain `FromStr` only.
//!
//! Entry point registers `Stop` param; `handle_stop_request` calls
//! `Stop::from_str` exclusively. Single resolver returning `Stop` →
//! detector does not fire.

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

pub fn handle_stop_request(stop: &str) -> Option<Stop> {
    Stop::from_str(stop)
}
