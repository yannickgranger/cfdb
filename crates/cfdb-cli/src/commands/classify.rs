use std::path::{Path, PathBuf};
use std::str::FromStr;

use cfdb_classify::ClassifyEnvelope;
use cfdb_query::{DiffEnvelope, ENVELOPE_SCHEMA_VERSION};

use crate::compose;
use crate::output;
use crate::output::OutputFormat;
use crate::scope::resolve_keyspace_name;

mod sorted_jsonl;
use sorted_jsonl::emit_sorted_jsonl;

#[allow(clippy::too_many_arguments)]
pub fn classify(
    db: PathBuf,
    keyspace: Option<String>,
    context: String,
    restrict_to_diff: PathBuf,
    output: Option<PathBuf>,
    workspace: Option<PathBuf>,
    format: String,
) -> Result<(), crate::CfdbCliError> {
    let format = OutputFormat::from_str(&format)?
        .require_one_of(&[OutputFormat::Json, OutputFormat::SortedJsonl], "classify")?;

    let ks_name = resolve_keyspace_name(&db, keyspace.as_deref())?;
    compose::ensure_keyspace_exists(&db, &ks_name)?;

    let diff_envelope = load_diff_envelope(&restrict_to_diff)?;

    let (store, ks) = match workspace {
        Some(ws) => compose::load_store_with_workspace(&db, &ks_name, Some(ws))?,
        None => compose::load_store(&db, &ks_name)?,
    };
    let engine = compose::classify_engine(&store);
    let envelope = engine.classify(&ks, &context, &diff_envelope)?;
    match format {
        OutputFormat::Json => emit_classify_output(&envelope, output.as_deref()),
        OutputFormat::SortedJsonl => emit_sorted_jsonl(&envelope, output.as_deref()),
        _ => unreachable!("classify allowlist is restricted to Json | SortedJsonl"),
    }
}

fn load_diff_envelope(path: &Path) -> Result<DiffEnvelope, crate::CfdbCliError> {
    let contents = std::fs::read_to_string(path)
        .map_err(|e| format!("read --restrict-to-diff file `{}`: {e}", path.display()))?;
    let env: DiffEnvelope = serde_json::from_str(&contents).map_err(|e| {
        format!(
            "parse --restrict-to-diff file `{}` as DiffEnvelope: {e}",
            path.display()
        )
    })?;
    if env.schema_version != ENVELOPE_SCHEMA_VERSION {
        return Err(format!(
            "diff envelope schema_version `{}` does not match expected `{}`",
            env.schema_version, ENVELOPE_SCHEMA_VERSION
        )
        .into());
    }
    Ok(env)
}

fn emit_classify_output(
    envelope: &ClassifyEnvelope,
    output_path: Option<&Path>,
) -> Result<(), crate::CfdbCliError> {
    match output_path {
        Some(path) => {
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        format!("create output parent dir `{}`: {e}", parent.display())
                    })?;
                }
            }
            let json = serde_json::to_string_pretty(envelope)?;
            std::fs::write(path, json)
                .map_err(|e| format!("write output `{}`: {e}", path.display()))?;
        }
        None => {
            output::emit_json(envelope)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn empty_diff(a: &str, b: &str) -> DiffEnvelope {
        DiffEnvelope {
            a: a.into(),
            b: b.into(),
            schema_version: ENVELOPE_SCHEMA_VERSION.into(),
            added: vec![],
            removed: vec![],
            changed: vec![],
            warnings: vec![],
        }
    }

    #[test]
    fn load_diff_envelope_rejects_bad_schema_version() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("diff.json");
        let bad = json!({
            "a": "a",
            "b": "b",
            "schema_version": "v999",
            "added": [],
            "removed": [],
        });
        std::fs::write(&path, serde_json::to_string(&bad).unwrap()).unwrap();
        let err = load_diff_envelope(&path).unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("schema_version"), "got: {msg}");
    }

    #[test]
    fn load_diff_envelope_reads_valid_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("diff.json");
        let env = empty_diff("x", "y");
        std::fs::write(&path, serde_json::to_string(&env).unwrap()).unwrap();
        let loaded = load_diff_envelope(&path).unwrap();
        assert_eq!(loaded.a, "x");
        assert_eq!(loaded.b, "y");
    }

    #[test]
    fn classify_format_allowlist_accepts_both_values() {
        let allow = [OutputFormat::Json, OutputFormat::SortedJsonl];
        assert_eq!(
            OutputFormat::from_str("json")
                .unwrap()
                .require_one_of(&allow, "classify")
                .unwrap(),
            OutputFormat::Json
        );
        assert_eq!(
            OutputFormat::from_str("sorted-jsonl")
                .unwrap()
                .require_one_of(&allow, "classify")
                .unwrap(),
            OutputFormat::SortedJsonl
        );
    }

    #[test]
    fn classify_format_allowlist_rejects_disallowed_with_enumerated_error() {
        let allow = [OutputFormat::Json, OutputFormat::SortedJsonl];
        let err = OutputFormat::from_str("text")
            .unwrap()
            .require_one_of(&allow, "classify")
            .unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("classify"), "got: {msg}");
        assert!(msg.contains("not supported"), "got: {msg}");
        assert!(msg.contains("json"), "got: {msg}");
        assert!(msg.contains("sorted-jsonl"), "got: {msg}");
        assert!(msg.contains("text"), "got: {msg}");
    }
}
