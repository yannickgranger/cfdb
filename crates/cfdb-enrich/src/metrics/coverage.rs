use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

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

pub(crate) fn parse_llvm_cov_json(json: &str) -> Result<BTreeMap<String, f64>, String> {
    let doc: LlvmCovDoc = serde_json::from_str(json).map_err(|e| format!("{e}"))?;
    Ok(doc
        .data
        .into_iter()
        .flat_map(|data_entry| data_entry.functions)
        .map(|func| {
            let ratio = (func.summary.lines.percent / 100.0).clamp(0.0, 1.0);
            (func.name, ratio)
        })
        .collect())
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
