//! `cfdb classify` — debt-class routing of `cfdb diff` findings.
//!
//! Reads a `DiffEnvelope` from `--restrict-to-diff`, runs the shared
//! classifier (`populate_findings_by_class_restricted` — delegates to
//! the same `populate_findings_by_class` that powers `cfdb scope`),
//! filters the resulting `findings_by_class` buckets to only items
//! whose `qname` appears in the diff, and emits a `ClassifyEnvelope`.
//!
//! Architecture: per RFC-cfdb.md §A2.2, the classifier is a query over
//! an enriched graph — `--db` + `--keyspace` are mandatory. Per §A2.3
//! + line 919, routing hints live external to `:Finding` rows; this
//!   handler emits only the structural `DebtClass` label.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use cfdb_query::{
    ClassifyEnvelope, DiffEnvelope, DiffSourceMeta, ScopeInventory, ENVELOPE_SCHEMA_VERSION,
};
use serde_json::Value;

use crate::commands::keyspace_path;
use crate::compose;
use crate::scope::{
    attach_scope_warnings, populate_findings_by_class_restricted, resolve_keyspace_name,
    validate_context, ExplainSink,
};

mod sorted_jsonl;
use sorted_jsonl::emit_sorted_jsonl;

/// Output surface for `cfdb classify`. `json` is the default pretty-printed
/// envelope (MVP from #213). `sorted-jsonl` emits one line per finding with
/// `{op, class, qname, ...}` — the line-diff-friendly analogue of
/// `cfdb diff --format sorted-jsonl` (#212).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ClassifyFormat {
    Json,
    SortedJsonl,
}

impl FromStr for ClassifyFormat {
    type Err = crate::CfdbCliError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        match raw {
            "json" => Ok(Self::Json),
            "sorted-jsonl" => Ok(Self::SortedJsonl),
            other => Err(crate::CfdbCliError::from(format!(
                "classify: --format `{other}` not supported; expected `json` or `sorted-jsonl`"
            ))),
        }
    }
}

/// Classify the findings-by-class subset that touches qnames in
/// `--restrict-to-diff`. See module docstring for the architecture
/// contract.
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
    let format = ClassifyFormat::from_str(&format)?;

    let ks_name = resolve_keyspace_name(&db, keyspace.as_deref())?;
    let ks_path = keyspace_path(&db, &ks_name);
    if !ks_path.exists() {
        return Err(format!(
            "keyspace `{ks_name}` not found in db `{}` (looked for {})",
            db.display(),
            ks_path.display()
        )
        .into());
    }

    let diff_envelope = load_diff_envelope(&restrict_to_diff)?;
    let restrict = collect_restrict_qnames(&diff_envelope);
    let diff_source = DiffSourceMeta {
        a: diff_envelope.a.clone(),
        b: diff_envelope.b.clone(),
        restrict_count: restrict.len() as u64,
    };

    let (store, ks) = match workspace {
        Some(ws) => compose::load_store_with_workspace(&db, &ks_name, Some(ws))?,
        None => compose::load_store(&db, &ks_name)?,
    };
    validate_context(&store, &ks, &context)?;

    let sink = ExplainSink::disabled();
    let mut inventory = ScopeInventory::new(&context, &ks_name);
    populate_findings_by_class_restricted(&store, &ks, &context, &restrict, &mut inventory, &sink)?;
    attach_scope_warnings(&mut inventory);

    let envelope = ClassifyEnvelope::new(inventory, diff_source);
    match format {
        ClassifyFormat::Json => emit_classify_output(&envelope, output.as_deref()),
        ClassifyFormat::SortedJsonl => emit_sorted_jsonl(&envelope, output.as_deref()),
    }
}

/// Read and deserialise a `DiffEnvelope` from disk. Enforces envelope
/// schema compatibility — an envelope whose `schema_version` does not
/// match `cfdb_query::ENVELOPE_SCHEMA_VERSION` is rejected so the
/// classify handler never silently consumes a future wire shape.
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

/// Derive the restrict-qname set from a `DiffEnvelope`. Includes every
/// node qname (props.qname with id fallback) from `added` and the two
/// envelope sides of `changed`, plus edge endpoint qnames (src_qname +
/// dst_qname) so classifier findings whose `:Item.qname` equals an edge
/// endpoint on a changed relationship are retained.
fn collect_restrict_qnames(env: &DiffEnvelope) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for fact in &env.added {
        extend_with_envelope_qnames(&mut out, &fact.envelope);
    }
    for fact in &env.changed {
        extend_with_envelope_qnames(&mut out, &fact.a);
        extend_with_envelope_qnames(&mut out, &fact.b);
    }
    out
}

fn extend_with_envelope_qnames(out: &mut BTreeSet<String>, envelope: &Value) {
    if let Some(kind) = envelope.get("kind").and_then(Value::as_str) {
        match kind {
            "node" => {
                // Prefer props.qname; fall back to id (matches
                // canonical_dump's sort-key resolution).
                if let Some(q) = envelope
                    .get("props")
                    .and_then(|p| p.get("qname"))
                    .and_then(Value::as_str)
                {
                    out.insert(q.to_string());
                } else if let Some(id) = envelope.get("id").and_then(Value::as_str) {
                    out.insert(id.to_string());
                }
            }
            "edge" => {
                if let Some(src) = envelope.get("src_qname").and_then(Value::as_str) {
                    out.insert(src.to_string());
                }
                if let Some(dst) = envelope.get("dst_qname").and_then(Value::as_str) {
                    out.insert(dst.to_string());
                }
            }
            _ => {}
        }
    }
}

/// Serialise the envelope to `output` (or stdout if `None`). Mirrors
/// `emit_scope_output` at `crates/cfdb-cli/src/scope.rs` in shape; kept
/// local to this module until a third caller justifies a generic emit.
fn emit_classify_output(
    envelope: &ClassifyEnvelope,
    output: Option<&Path>,
) -> Result<(), crate::CfdbCliError> {
    let json = serde_json::to_string_pretty(envelope)?;
    match output {
        Some(path) => {
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        format!("create output parent dir `{}`: {e}", parent.display())
                    })?;
                }
            }
            std::fs::write(path, json)
                .map_err(|e| format!("write output `{}`: {e}", path.display()))?;
        }
        None => {
            println!("{json}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cfdb_query::{ChangedFact, DiffFact};
    use serde_json::json;

    fn node_envelope(qname: &str) -> Value {
        json!({
            "id": format!("item:{qname}"),
            "kind": "node",
            "label": "Item",
            "props": {"qname": qname},
        })
    }

    fn edge_envelope(src: &str, dst: &str) -> Value {
        json!({
            "dst_qname": dst,
            "kind": "edge",
            "label": "CALLS",
            "props": {},
            "src_qname": src,
        })
    }

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
    fn collect_restrict_qnames_extracts_added_node_props_qname() {
        let mut env = empty_diff("a", "b");
        env.added.push(DiffFact {
            kind: "node".into(),
            envelope: node_envelope("foo::Bar"),
        });
        let set = collect_restrict_qnames(&env);
        assert!(set.contains("foo::Bar"));
        assert_eq!(set.len(), 1);
    }

    #[test]
    fn collect_restrict_qnames_falls_back_to_id_when_props_qname_absent() {
        let mut env = empty_diff("a", "b");
        env.added.push(DiffFact {
            kind: "node".into(),
            envelope: json!({
                "id": "callsite:abc123",
                "kind": "node",
                "label": "CallSite",
                "props": {},
            }),
        });
        let set = collect_restrict_qnames(&env);
        assert!(set.contains("callsite:abc123"));
    }

    #[test]
    fn collect_restrict_qnames_includes_edge_endpoints() {
        let mut env = empty_diff("a", "b");
        env.added.push(DiffFact {
            kind: "edge".into(),
            envelope: edge_envelope("caller::fn", "callee::fn"),
        });
        let set = collect_restrict_qnames(&env);
        assert!(set.contains("caller::fn"));
        assert!(set.contains("callee::fn"));
    }

    #[test]
    fn collect_restrict_qnames_includes_both_sides_of_changed() {
        let mut env = empty_diff("a", "b");
        env.changed.push(ChangedFact {
            kind: "node".into(),
            a: node_envelope("old::Name"),
            b: node_envelope("new::Name"),
        });
        let set = collect_restrict_qnames(&env);
        assert!(set.contains("old::Name"));
        assert!(set.contains("new::Name"));
    }

    #[test]
    fn collect_restrict_qnames_unions_added_and_changed() {
        let mut env = empty_diff("a", "b");
        env.added.push(DiffFact {
            kind: "node".into(),
            envelope: node_envelope("added::X"),
        });
        env.changed.push(ChangedFact {
            kind: "node".into(),
            a: node_envelope("changed::Y"),
            b: node_envelope("changed::Y"),
        });
        env.added.push(DiffFact {
            kind: "edge".into(),
            envelope: edge_envelope("edge::src", "edge::dst"),
        });
        let set = collect_restrict_qnames(&env);
        assert_eq!(set.len(), 4);
        for q in ["added::X", "changed::Y", "edge::src", "edge::dst"] {
            assert!(set.contains(q), "missing {q}");
        }
    }

    #[test]
    fn collect_restrict_qnames_empty_diff_produces_empty_set() {
        let env = empty_diff("a", "b");
        assert!(collect_restrict_qnames(&env).is_empty());
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
    fn classify_format_from_str_accepts_both_values() {
        assert_eq!(
            ClassifyFormat::from_str("json").unwrap(),
            ClassifyFormat::Json
        );
        assert_eq!(
            ClassifyFormat::from_str("sorted-jsonl").unwrap(),
            ClassifyFormat::SortedJsonl
        );
    }

    #[test]
    fn classify_format_from_str_rejects_unknown_with_enumerated_error() {
        let err = ClassifyFormat::from_str("toml").unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("json"), "got: {msg}");
        assert!(msg.contains("sorted-jsonl"), "got: {msg}");
        assert!(msg.contains("toml"), "got: {msg}");
    }
}
