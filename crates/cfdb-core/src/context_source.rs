//! `ContextSource` — provenance discriminator for `:Context` nodes (RFC-038).
//!
//! `Declared` contexts are author-asserted in `.cfdb/concepts/<name>.toml`.
//! `Heuristic` contexts are auto-derived from crate-name prefix stripping in
//! `cfdb_concepts::compute_bounded_context`. The wire format is the
//! lower-case variant name; round-trips through `:Context.source` prop via
//! `FromStr`/`Display`.
//!
//! ## Closed-set wire-enum convention
//!
//! `ContextSource` has no variant carrying owned data. `as_wire_str` returns
//! `&'static str` directly — no allocation. This is the closed-set convention
//! captured in RFC-038 §3.1 (open-set wire enums like `Visibility::Restricted(String)`
//! return `String` because their variants own data; this enum doesn't, so it
//! returns the static literal).

use std::fmt;
use std::str::FromStr;

/// Provenance discriminator for `:Context` nodes (RFC-038). `Declared` is
/// author-asserted in `.cfdb/concepts/<name>.toml`; `Heuristic` is auto-derived
/// by `cfdb_concepts::compute_bounded_context` via crate-name prefix stripping.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ContextSource {
    Declared,
    Heuristic,
}

impl ContextSource {
    /// Canonical wire string. Round-trips through `:Context.source` prop.
    pub fn as_wire_str(&self) -> &'static str {
        match self {
            ContextSource::Declared => "declared",
            ContextSource::Heuristic => "heuristic",
        }
    }
}

impl fmt::Display for ContextSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_wire_str())
    }
}

impl FromStr for ContextSource {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "declared" => Ok(ContextSource::Declared),
            "heuristic" => Ok(ContextSource::Heuristic),
            other => Err(format!("unrecognised context source: {other:?}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_declared() {
        assert_eq!(ContextSource::Declared.as_wire_str(), "declared");
        assert_eq!(
            "declared".parse::<ContextSource>().unwrap(),
            ContextSource::Declared
        );
        assert_eq!(format!("{}", ContextSource::Declared), "declared");
    }

    #[test]
    fn round_trip_heuristic() {
        assert_eq!(ContextSource::Heuristic.as_wire_str(), "heuristic");
        assert_eq!(
            "heuristic".parse::<ContextSource>().unwrap(),
            ContextSource::Heuristic
        );
        assert_eq!(format!("{}", ContextSource::Heuristic), "heuristic");
    }

    #[test]
    fn unknown_wire_string_rejects_with_error_message() {
        let err = "unknown".parse::<ContextSource>().unwrap_err();
        assert!(err.contains("unknown"), "error should mention input: {err}");
        assert!(
            err.contains("unrecognised"),
            "error should say 'unrecognised': {err}"
        );
    }

    #[test]
    fn empty_string_rejects() {
        assert!("".parse::<ContextSource>().is_err());
    }

    #[test]
    fn case_sensitive() {
        // Wire format is canonically lower-case; mixed-case rejects.
        assert!("Declared".parse::<ContextSource>().is_err());
        assert!("DECLARED".parse::<ContextSource>().is_err());
    }
}
