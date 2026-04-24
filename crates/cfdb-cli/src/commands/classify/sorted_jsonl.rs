//! Sorted-JSONL rendering for `cfdb classify --format sorted-jsonl`.
//!
//! Extracted from `commands/classify.rs` (epic #248 — god-file split).
//! The `Command::Classify` dispatch and all non-sorted-jsonl logic remain
//! in the parent module.

use std::path::Path;

use cfdb_query::ClassifyEnvelope;
use serde_json::Value;

/// Build the `{"op":"header", ...}` first line of a sorted-jsonl dump.
/// Carries the envelope's scalar metadata (schema_version, inventory
/// context + keyspace_sha, diff_source triple). Alphabetical key order
/// is provided by `Value::Object`'s `BTreeMap` backing (RFC-cfdb.md
/// §12.1 G1 — no `HashMap`, no wall-clock).
pub(super) fn build_sorted_jsonl_header(envelope: &ClassifyEnvelope) -> Value {
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
pub(super) fn render_sorted_jsonl(
    envelope: &ClassifyEnvelope,
) -> Result<String, serde_json::Error> {
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
pub(super) fn emit_sorted_jsonl(
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
    use cfdb_query::{
        ClassifyEnvelope, DebtClass, DiffSourceMeta, Finding, ScopeInventory,
        CLASSIFY_ENVELOPE_SCHEMA_VERSION,
    };
    use serde_json::json;

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
