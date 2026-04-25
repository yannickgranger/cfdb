//! Tiny output helpers — keep stdout shape consistent across handlers.

use std::str::FromStr;

use serde::Serialize;

use crate::CfdbCliError;

/// Pretty-print `payload` as JSON to stdout (newline-terminated via println!).
/// Centralises the `serde_json::to_string_pretty + println!` shape that every
/// JSON-emitting handler used to inline. Reachable from the binary crate
/// (`main_dispatch.rs`) via the crate-root `pub use` re-export, same pattern
/// as the other handler exports.
pub fn emit_json<T: Serialize + ?Sized>(payload: &T) -> Result<(), CfdbCliError> {
    let json = serde_json::to_string_pretty(payload)?;
    println!("{json}");
    Ok(())
}

/// Canonical output format flag for every cfdb subcommand that takes
/// `--format`. Each handler accepts a subset of variants — see per-site
/// allowlist via [`OutputFormat::require_one_of`].
///
/// Wire strings (`text`, `json`, `sorted-jsonl`, `table`) are stable and
/// asserted on by integration tests; do not rename them without a
/// deliberate user-facing change. See EPIC #273 Pattern 1 #4.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    /// `text` — TSV / human-readable shape (e.g. `cfdb check-predicate --format text`).
    Text,
    /// `json` — pretty-printed JSON envelope, the default for most verbs.
    Json,
    /// `sorted-jsonl` — one JSON object per line, deterministic sort order.
    SortedJsonl,
    /// `table` — reserved for v0.2 tabular output. No handler accepts this today.
    Table,
}

impl OutputFormat {
    /// Wire string used on the command line. Round-trips with [`FromStr`].
    pub fn as_wire(&self) -> &'static str {
        match self {
            OutputFormat::Text => "text",
            OutputFormat::Json => "json",
            OutputFormat::SortedJsonl => "sorted-jsonl",
            OutputFormat::Table => "table",
        }
    }

    /// Allowlist check — return `Ok(self)` if `self` is one of `allowed`,
    /// otherwise produce a [`CfdbCliError::Usage`] of the shape
    /// `"<cmd>: --format `<got>` not supported; expected `<a>` or `<b>` ..."`.
    /// The wire shape matches the per-handler error messages that existed
    /// before unification (see EPIC #273 Pattern 1 #4) so substring-asserting
    /// integration tests keep passing.
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
