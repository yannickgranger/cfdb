//! `cargo llvm-cov --json` output → per-qname line-coverage ratio.
//!
//! `cargo-llvm-cov` emits LLVM's standard `llvm-cov export` JSON with
//! `data[].functions[]` entries carrying `name` + `count` + `regions`.
//! We consume the `summary` block at `data[].functions[].summary.lines`
//! which carries `{count, covered, percent}` — percent is already in
//! [0.0, 100.0], so divide by 100 for the [0.0, 1.0] ratio the schema
//! expects.
//!
//! Function-name → qname mapping: `name` from llvm-cov is the
//! mangled-or-demangled Rust symbol (e.g.
//! `cfdb_core::enrich::EnrichReport::not_implemented`). We match on
//! suffix against the known qname set — if the target workspace was
//! compiled with symbol mangling, the coverage mapper is a separate
//! concern; for the dogfood path `cargo llvm-cov` unmangles by default.

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

/// Load coverage ratios from a cargo-llvm-cov JSON file. Missing file
/// or parse errors push a warning and return an empty map — never
/// panics.
pub(crate) fn load_from_path(path: &Path, warnings: &mut Vec<String>) -> BTreeMap<String, f64> {
    let src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            warnings.push(format!(
                "{}: failed to read coverage JSON at {}: {e}",
                super::VERB,
                path.display()
            ));
            return BTreeMap::new();
        }
    };
    match parse_llvm_cov_json(&src) {
        Ok(m) => m,
        Err(e) => {
            warnings.push(format!(
                "{}: failed to parse coverage JSON at {}: {e}",
                super::VERB,
                path.display()
            ));
            BTreeMap::new()
        }
    }
}

/// Parse an `llvm-cov export --format=text` JSON blob into
/// `qname → coverage_ratio` map (ratio in [0.0, 1.0]). `pub(crate)` —
/// consumed by `load_from_path` + unit tests.
pub(crate) fn parse_llvm_cov_json(json: &str) -> Result<BTreeMap<String, f64>, String> {
    let doc: LlvmCovDoc = serde_json::from_str(json).map_err(|e| format!("{e}"))?;
    let mut out: BTreeMap<String, f64> = BTreeMap::new();
    for data_entry in &doc.data {
        for func in &data_entry.functions {
            let ratio = (func.summary.lines.percent / 100.0).clamp(0.0, 1.0);
            out.insert(func.name.clone(), ratio);
        }
    }
    Ok(out)
}

#[derive(Deserialize)]
struct LlvmCovDoc {
    #[serde(default)]
    data: Vec<DataEntry>,
}

#[derive(Deserialize)]
struct DataEntry {
    #[serde(default)]
    functions: Vec<Function>,
}

#[derive(Deserialize)]
struct Function {
    name: String,
    summary: FunctionSummary,
}

#[derive(Deserialize)]
struct FunctionSummary {
    lines: LinesSummary,
}

#[derive(Deserialize)]
struct LinesSummary {
    percent: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_llvm_cov_blob() {
        let json = r#"{
            "data": [{
                "functions": [
                    {"name": "crate::a::foo", "summary": {"lines": {"percent": 75.0}}},
                    {"name": "crate::b::bar", "summary": {"lines": {"percent": 100.0}}},
                    {"name": "crate::c::baz", "summary": {"lines": {"percent": 0.0}}}
                ]
            }]
        }"#;
        let m = parse_llvm_cov_json(json).expect("parses");
        assert_eq!(m.get("crate::a::foo"), Some(&0.75));
        assert_eq!(m.get("crate::b::bar"), Some(&1.0));
        assert_eq!(m.get("crate::c::baz"), Some(&0.0));
    }

    #[test]
    fn empty_data_array_yields_empty_map() {
        let json = r#"{"data": []}"#;
        let m = parse_llvm_cov_json(json).expect("parses");
        assert!(m.is_empty());
    }

    #[test]
    fn malformed_json_returns_err() {
        assert!(parse_llvm_cov_json("not json").is_err());
    }

    #[test]
    fn percent_over_100_clamps_to_1() {
        // Defensive: llvm-cov in theory never emits >100, but clamp is cheap.
        let json =
            r#"{"data":[{"functions":[{"name":"x","summary":{"lines":{"percent":150.0}}}]}]}"#;
        let m = parse_llvm_cov_json(json).expect("parses");
        assert_eq!(m.get("x"), Some(&1.0));
    }

    #[test]
    fn negative_percent_clamps_to_zero() {
        let json =
            r#"{"data":[{"functions":[{"name":"x","summary":{"lines":{"percent":-1.0}}}]}]}"#;
        let m = parse_llvm_cov_json(json).expect("parses");
        assert_eq!(m.get("x"), Some(&0.0));
    }
}
