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

/// Build the `{"op":"header", ...}` first line of a sorted-jsonl dump.
/// Carries the envelope's scalar metadata (schema_version, inventory
/// context + keyspace_sha, diff_source triple). Alphabetical key order
/// is provided by `Value::Object`'s `BTreeMap` backing (RFC-cfdb.md
/// §12.1 G1 — no `HashMap`, no wall-clock).
fn build_sorted_jsonl_header(envelope: &ClassifyEnvelope) -> Value {
    serde_json::json!({
        "op": "header",
        "schema_version": envelope.schema_version,
        "inventory_context": envelope.inventory.context,
        "keyspace_sha": envelope.inventory.keyspace_sha,
        "diff_source": {
            "a": envelope.diff_source.a,
            "b": envelope.diff_source.b,
            "restrict_count": envelope.diff_source.restrict_count,
        },
    })
}

/// Render the sorted-JSONL body (header + sorted finding lines + warning
/// lines) as a single string — separated by LF, no trailing newline. Pure
/// function so Gate 3 unit tests can assert byte-stable output without a
/// tempfile / stdout capture.
fn render_sorted_jsonl(envelope: &ClassifyEnvelope) -> Result<String, serde_json::Error> {
    let mut lines: Vec<String> = Vec::new();

    lines.push(serde_json::to_string(&build_sorted_jsonl_header(envelope))?);

    for (class, class_findings) in &envelope.inventory.findings_by_class {
        let mut findings: Vec<_> = class_findings.iter().collect();
        findings.sort();
        for finding in &findings {
            let row = serde_json::json!({
                "op": "finding",
                "class": class.as_str(),
                "qname": finding.qname,
                "name": finding.name,
                "kind": finding.kind,
                "crate": finding.crate_name,
                "file": finding.file,
                "line": finding.line,
                "bounded_context": finding.bounded_context,
            });
            lines.push(serde_json::to_string(&row)?);
        }
    }

    for warning in &envelope.inventory.warnings {
        let row = serde_json::json!({"op": "warning", "message": warning});
        lines.push(serde_json::to_string(&row)?);
    }

    Ok(lines.join("\n"))
}

/// Emit a `ClassifyEnvelope` as sorted-JSONL — one JSON object per line,
/// header first, then per-finding lines ordered by `(DebtClass::Ord,
/// Finding::Ord)`, then per-warning lines in envelope order.
///
/// Mirrors `cfdb diff --format sorted-jsonl` (`commands/diff.rs:85-110`)
/// — the cfdb-wide convention for sorted-JSONL output carrying envelope
/// scalars is a `{"op":"header",…}` first line, per-row `op: finding|
/// warning` discriminators. See `.prescriptions/236.md` CREATE-1 for the
/// exact per-line shapes.
fn emit_sorted_jsonl(
    envelope: &ClassifyEnvelope,
    output: Option<&Path>,
) -> Result<(), crate::CfdbCliError> {
    let body = render_sorted_jsonl(envelope)?;
    match output {
        Some(path) => {
            if let Some(parent) = path.parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        format!("create output parent dir `{}`: {e}", parent.display())
                    })?;
                }
            }
            std::fs::write(path, &body)
                .map_err(|e| format!("write output `{}`: {e}", path.display()))?;
        }
        None => {
            println!("{body}");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cfdb_query::{ChangedFact, DebtClass, DiffFact, Finding, CLASSIFY_ENVELOPE_SCHEMA_VERSION};
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

    // ── sorted-jsonl branch tests (#236) ────────────────────────────────────

    fn finding(qname: &str, name: &str, crate_name: &str) -> Finding {
        Finding {
            qname: qname.into(),
            name: name.into(),
            kind: "fn".into(),
            crate_name: crate_name.into(),
            file: "src/lib.rs".into(),
            line: 1,
            bounded_context: "cfdb".into(),
        }
    }

    fn envelope_with(findings: Vec<(DebtClass, Finding)>) -> ClassifyEnvelope {
        let mut inv = ScopeInventory::new("cfdb", "keyspace-sha-12");
        for (class, f) in findings {
            inv.findings_by_class
                .get_mut(&class)
                .expect("class pre-seeded")
                .push(f);
        }
        ClassifyEnvelope::new(
            inv,
            DiffSourceMeta {
                a: "cfdb-a".into(),
                b: "cfdb-b".into(),
                restrict_count: 3,
            },
        )
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

    #[test]
    fn sorted_jsonl_header_line_shape() {
        let env = envelope_with(vec![]);
        let header = build_sorted_jsonl_header(&env);
        let obj = header.as_object().expect("header is an object");
        // Alphabetical key order, per Value::Object BTreeMap backing.
        let keys: Vec<&String> = obj.keys().collect();
        assert_eq!(
            keys,
            vec![
                "diff_source",
                "inventory_context",
                "keyspace_sha",
                "op",
                "schema_version"
            ]
        );
        assert_eq!(header["op"], "header");
        assert_eq!(header["schema_version"], CLASSIFY_ENVELOPE_SCHEMA_VERSION);
        assert_eq!(header["inventory_context"], "cfdb");
        assert_eq!(header["keyspace_sha"], "keyspace-sha-12");
        assert_eq!(header["diff_source"]["a"], "cfdb-a");
        assert_eq!(header["diff_source"]["b"], "cfdb-b");
        assert_eq!(header["diff_source"]["restrict_count"], 3);
    }

    #[test]
    fn sorted_jsonl_sort_key_is_class_then_finding_ord() {
        // Two classes × two findings (shuffled). DebtClass::variants() order
        // is {Duplicated, ContextHomonym, UnfinishedRefactor, RandomScattering,
        // CanonicalBypass, Unwired}. For BTreeMap iteration, `Ord` derives
        // declaration order (same list). Within each class, Finding::Ord
        // sorts by qname first (derivation order).
        let env = envelope_with(vec![
            (DebtClass::Unwired, finding("z::late", "late", "crate-z")),
            (
                DebtClass::DuplicatedFeature,
                finding("b::mid", "mid", "crate-b"),
            ),
            (
                DebtClass::DuplicatedFeature,
                finding("a::early", "early", "crate-a"),
            ),
            (
                DebtClass::Unwired,
                finding("y::before", "before", "crate-y"),
            ),
        ]);

        let body = render_sorted_jsonl(&env).unwrap();
        let lines: Vec<&str> = body.lines().collect();
        assert_eq!(lines.len(), 5); // header + 4 findings

        // Skip header (line 0); parse finding lines.
        let pairs: Vec<(String, String)> = lines[1..]
            .iter()
            .map(|l| {
                let v: Value = serde_json::from_str(l).expect("finding line parses");
                assert_eq!(v["op"], "finding");
                (
                    v["class"].as_str().unwrap().to_string(),
                    v["qname"].as_str().unwrap().to_string(),
                )
            })
            .collect();

        assert_eq!(
            pairs,
            vec![
                ("duplicated_feature".into(), "a::early".into()),
                ("duplicated_feature".into(), "b::mid".into()),
                ("unwired".into(), "y::before".into()),
                ("unwired".into(), "z::late".into()),
            ]
        );
    }

    #[test]
    fn sorted_jsonl_determinism_byte_stable() {
        let env = envelope_with(vec![
            (DebtClass::DuplicatedFeature, finding("a", "a", "k")),
            (DebtClass::CanonicalBypass, finding("b", "b", "k")),
            (DebtClass::Unwired, finding("c", "c", "k")),
        ]);
        let a = render_sorted_jsonl(&env).unwrap();
        let b = render_sorted_jsonl(&env).unwrap();
        assert_eq!(a, b, "two renderings must be byte-identical");
    }

    #[test]
    fn sorted_jsonl_has_no_trailing_newline_on_file_write() {
        let env = envelope_with(vec![(DebtClass::DuplicatedFeature, finding("x", "x", "k"))]);
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("out.jsonl");
        emit_sorted_jsonl(&env, Some(&path)).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_ne!(
            *bytes.last().unwrap(),
            b'\n',
            "sorted-jsonl file write MUST NOT have trailing newline (RFC §12.1)"
        );
    }

    #[test]
    fn sorted_jsonl_finding_line_uses_crate_not_crate_name() {
        let env = envelope_with(vec![(
            DebtClass::DuplicatedFeature,
            finding("x", "x", "my-crate"),
        )]);
        let body = render_sorted_jsonl(&env).unwrap();
        let finding_line = body.lines().nth(1).expect("finding line present");
        assert!(
            finding_line.contains("\"crate\":\"my-crate\""),
            "expected serde-renamed `crate` key, got: {finding_line}"
        );
        assert!(
            !finding_line.contains("crate_name"),
            "sorted-jsonl MUST use `crate` (matches json format), got: {finding_line}"
        );
    }

    #[test]
    fn sorted_jsonl_warning_line_shape() {
        use cfdb_core::result::{Warning, WarningKind};
        let mut env = envelope_with(vec![]);
        env.inventory.warnings.push(Warning {
            kind: WarningKind::EmptyResult,
            message: "no findings for class `unwired`".into(),
            suggestion: None,
        });
        let body = render_sorted_jsonl(&env).unwrap();
        let warning_line = body.lines().nth(1).expect("warning line after header");
        let v: Value = serde_json::from_str(warning_line).expect("warning line parses");
        assert_eq!(v["op"], "warning");
        let obj = v.as_object().unwrap();
        // Alphabetical keys: "message" < "op".
        assert_eq!(
            obj.keys().collect::<Vec<_>>(),
            vec!["message", "op"],
            "warning line keys must be alphabetical"
        );
    }

    #[test]
    fn sorted_jsonl_empty_inventory_emits_only_header() {
        let env = envelope_with(vec![]);
        let body = render_sorted_jsonl(&env).unwrap();
        assert_eq!(body.lines().count(), 1, "expected only the header line");
        let header: Value = serde_json::from_str(&body).expect("header parses");
        assert_eq!(header["op"], "header");
    }
}
