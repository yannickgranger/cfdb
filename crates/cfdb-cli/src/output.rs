use std::str::FromStr;

use serde::Serialize;

use crate::CfdbCliError;

pub fn emit_json<T: Serialize + ?Sized>(payload: &T) -> Result<(), CfdbCliError> {
    let json = serde_json::to_string_pretty(payload)?;
    println!("{json}");
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Text,
    Json,
    SortedJsonl,
    Table,
}

impl OutputFormat {
    pub fn as_wire(&self) -> &'static str {
        match self {
            OutputFormat::Text => "text",
            OutputFormat::Json => "json",
            OutputFormat::SortedJsonl => "sorted-jsonl",
            OutputFormat::Table => "table",
        }
    }

    pub fn require_one_of(self, allowed: &[OutputFormat], cmd: &str) -> Result<Self, CfdbCliError> {
        if allowed.contains(&self) {
            return Ok(self);
        }
        let names: Vec<String> = allowed
            .iter()
            .map(|f| format!("`{}`", f.as_wire()))
            .collect();
        let expected = names.join(" or ");
        Err(CfdbCliError::from(format!(
            "{cmd}: --format `{}` not supported; expected {expected}",
            self.as_wire()
        )))
    }
}

impl FromStr for OutputFormat {
    type Err = CfdbCliError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "text" => Ok(OutputFormat::Text),
            "json" => Ok(OutputFormat::Json),
            "sorted-jsonl" => Ok(OutputFormat::SortedJsonl),
            "table" => Ok(OutputFormat::Table),
            other => Err(CfdbCliError::from(format!(
                "--format `{other}` not supported; expected one of: text, json, sorted-jsonl, table"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_str_round_trips_each_wire_variant() {
        for variant in [
            OutputFormat::Text,
            OutputFormat::Json,
            OutputFormat::SortedJsonl,
            OutputFormat::Table,
        ] {
            assert_eq!(
                OutputFormat::from_str(variant.as_wire()).unwrap(),
                variant,
                "wire string `{}` did not round-trip",
                variant.as_wire()
            );
        }
    }

    #[test]
    fn from_str_rejects_unknown_with_enumerated_message() {
        let err = OutputFormat::from_str("toml").unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("toml"), "got: {msg}");
        assert!(msg.contains("text"), "got: {msg}");
        assert!(msg.contains("json"), "got: {msg}");
        assert!(msg.contains("sorted-jsonl"), "got: {msg}");
        assert!(msg.contains("table"), "got: {msg}");
    }

    #[test]
    fn require_one_of_accepts_allowed_variant() {
        let got = OutputFormat::Json
            .require_one_of(&[OutputFormat::Json, OutputFormat::SortedJsonl], "diff")
            .unwrap();
        assert_eq!(got, OutputFormat::Json);
    }

    #[test]
    fn require_one_of_rejects_disallowed_with_cmd_prefix_and_wire_list() {
        let err = OutputFormat::Text
            .require_one_of(&[OutputFormat::Json, OutputFormat::SortedJsonl], "diff")
            .unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("diff"), "got: {msg}");
        assert!(msg.contains("not supported"), "got: {msg}");
        assert!(msg.contains("text"), "got: {msg}");
        assert!(msg.contains("json"), "got: {msg}");
        assert!(msg.contains("sorted-jsonl"), "got: {msg}");
    }
}
