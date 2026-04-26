//! Self-dogfood scar — RFC-038 issue #302 final slice. Extracts cfdb's own
//! sub-workspace via `extract_workspace` and asserts every emitted
//! `:Context` node carries a valid `source` prop whose value matches the
//! provenance the actual `.cfdb/concepts/*.toml` file set declares at
//! slice-ship time.
//!
//! Coverage at slice-ship time:
//! - cfdb's `.cfdb/concepts/cfdb.toml` declares ALL workspace crates under
//!   the single `cfdb` bounded context. Therefore every emitted `:Context`
//!   on cfdb's own tree resolves to `source = "declared"`.
//! - There are NO heuristic-source `:Context` nodes on cfdb's own tree.
//!   The four-case unit test in `lib.rs::context_source_aggregation_tests`
//!   exercises the heuristic path with a synthetic workspace fixture
//!   instead; this scar covers the real-source-tree dogfood.

use std::collections::BTreeSet;
use std::path::Path;

use cfdb_core::fact::PropValue;
use cfdb_core::schema::Label;
use cfdb_core::ContextSource;
use cfdb_extractor::extract_workspace;

/// Resolve the cfdb sub-workspace root — this crate's grandparent directory.
fn cfdb_workspace_root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cfdb-extractor crate dir has parent (crates/)")
        .parent()
        .expect("crates/ has parent (cfdb sub-workspace root)")
}

/// Read every `.cfdb/concepts/*.toml` and collect the `name` field. This is
/// the set of context names the override declares; every other emitted
/// `:Context` on cfdb's tree must therefore be heuristic-sourced.
fn read_declared_context_names(workspace_root: &Path) -> BTreeSet<String> {
    let dir = workspace_root.join(".cfdb").join("concepts");
    let mut out = BTreeSet::new();
    let entries = std::fs::read_dir(&dir).expect("concepts/ readable");
    for e in entries {
        let path = e.expect("dir entry").path();
        if path.extension().and_then(|s| s.to_str()) != Some("toml") {
            continue;
        }
        let body = std::fs::read_to_string(&path).expect("read toml");
        // The file's `name = "<context>"` line is the canonical context
        // identifier — same field `cfdb_concepts::ConceptFile` deserialises.
        for line in body.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("name") {
                let v = rest
                    .trim_start()
                    .strip_prefix('=')
                    .map(str::trim)
                    .and_then(|s| s.strip_prefix('"'))
                    .and_then(|s| s.strip_suffix('"'));
                if let Some(name) = v {
                    out.insert(name.to_string());
                    break;
                }
            }
        }
    }
    out
}

#[test]
fn every_context_node_carries_valid_source_prop() {
    let root = cfdb_workspace_root();
    let (nodes, _edges) = extract_workspace(root).expect("extract cfdb sub-workspace");

    let context_nodes: Vec<_> = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::CONTEXT)
        .collect();
    assert!(
        !context_nodes.is_empty(),
        "expected at least one :Context node on cfdb's own tree"
    );

    for node in &context_nodes {
        let raw = node.props.get("source");
        let parsed = match raw {
            Some(PropValue::Str(s)) => s.parse::<ContextSource>().ok(),
            _ => None,
        };
        assert!(
            parsed.is_some(),
            ":Context node {} missing or invalid `source` prop: {:?}",
            node.id,
            raw
        );
    }
}

#[test]
fn context_source_matches_declared_toml_set() {
    let root = cfdb_workspace_root();
    let (nodes, _edges) = extract_workspace(root).expect("extract cfdb sub-workspace");

    let declared_names = read_declared_context_names(root);
    assert!(
        !declared_names.is_empty(),
        ".cfdb/concepts/*.toml declared at least one context — read_declared_context_names\n\
         returned empty, parser broken or fixture missing"
    );

    let mut declared_count = 0usize;
    let mut heuristic_count = 0usize;

    for node in nodes.iter().filter(|n| n.label.as_str() == Label::CONTEXT) {
        let name = node
            .props
            .get("name")
            .and_then(PropValue::as_str)
            .expect(":Context node missing required `name` prop");
        let source: ContextSource = node
            .props
            .get("source")
            .and_then(PropValue::as_str)
            .expect(":Context node missing required `source` prop")
            .parse()
            .expect("source prop must be valid wire string");

        // Strong assertion: source value must match TOML provenance for the
        // ACTUAL `.cfdb/concepts/*.toml` file set committed at slice-ship
        // time. If the name is in the declared set, source MUST be declared;
        // otherwise it MUST be heuristic.
        let expected = if declared_names.contains(name) {
            ContextSource::Declared
        } else {
            ContextSource::Heuristic
        };
        assert_eq!(
            source, expected,
            ":Context name={name} has source={source}, expected {expected} \
             (declared TOML names: {declared_names:?})"
        );

        match source {
            ContextSource::Declared => declared_count += 1,
            ContextSource::Heuristic => heuristic_count += 1,
        }
    }

    // Coverage assertion (weakened per #302 prescription escape hatch):
    // cfdb's `.cfdb/concepts/cfdb.toml` declares ALL workspace crates under
    // the single `cfdb` context, so the cfdb tree emits zero heuristic
    // contexts. The four-case unit test in lib.rs covers the heuristic
    // path against a synthetic fixture; here we assert at least the
    // declared variant is present. Heuristic count is reported for the PR
    // body sanity-check, not asserted.
    assert!(
        declared_count >= 1,
        "expected >=1 declared :Context on cfdb's tree, got {declared_count}"
    );
    eprintln!(
        "self-dogfood :Context source distribution: declared={declared_count} heuristic={heuristic_count}"
    );
}
