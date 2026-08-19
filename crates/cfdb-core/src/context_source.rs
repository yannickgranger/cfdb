use std::fmt;
use std::str::FromStr;

use crate::fact::PropValue;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ContextSource {
    Declared,
    Heuristic,
}

impl ContextSource {
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

#[must_use]
pub fn parse_or_default(prop_value: Option<&PropValue>) -> ContextSource {
    match prop_value {
        Some(PropValue::Str(s)) => s.parse().unwrap_or(ContextSource::Heuristic),
        _ => ContextSource::Heuristic,
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
        assert!("Declared".parse::<ContextSource>().is_err());
        assert!("DECLARED".parse::<ContextSource>().is_err());
    }

    #[test]
    fn parse_or_default_absent_returns_heuristic() {
        assert_eq!(parse_or_default(None), ContextSource::Heuristic);
    }

    #[test]
    fn parse_or_default_string_declared() {
        let v = PropValue::Str("declared".into());
        assert_eq!(parse_or_default(Some(&v)), ContextSource::Declared);
    }

    #[test]
    fn parse_or_default_string_heuristic() {
        let v = PropValue::Str("heuristic".into());
        assert_eq!(parse_or_default(Some(&v)), ContextSource::Heuristic);
    }

    #[test]
    fn parse_or_default_string_invalid_returns_heuristic() {
        let v = PropValue::Str("garbage".into());
        assert_eq!(parse_or_default(Some(&v)), ContextSource::Heuristic);
    }

    #[test]
    fn parse_or_default_null_returns_heuristic() {
        let v = PropValue::Null;
        assert_eq!(parse_or_default(Some(&v)), ContextSource::Heuristic);
    }

    #[test]
    fn parse_or_default_int_returns_heuristic() {
        let v = PropValue::Int(0);
        assert_eq!(parse_or_default(Some(&v)), ContextSource::Heuristic);
    }

    #[test]
    fn parse_or_default_float_returns_heuristic() {
        let v = PropValue::Float(0.0);
        assert_eq!(parse_or_default(Some(&v)), ContextSource::Heuristic);
    }

    #[test]
    fn parse_or_default_bool_returns_heuristic() {
        let v = PropValue::Bool(false);
        assert_eq!(parse_or_default(Some(&v)), ContextSource::Heuristic);
    }
}
