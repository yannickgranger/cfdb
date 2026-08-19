use std::collections::BTreeMap;
use std::path::Path;

use cfdb_core::fact::{Node, PropValue};
use cfdb_core::schema::Label;
use cfdb_extractor::extract_workspace;
use sha2::{Digest, Sha256};
use tempfile::tempdir;

fn write_fixture_file(root: &Path, rel: &str, contents: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().expect("fixture path has parent")).expect("mkdir -p");
    std::fs::write(p, contents).expect("write fixture");
}

fn write_cargo_workspace(root: &Path, crate_name: &str, lib_src: &str) {
    write_fixture_file(
        root,
        "Cargo.toml",
        &format!(
            r#"[workspace]
resolver = "2"
members = ["{crate_name}"]
"#
        ),
    );
    write_fixture_file(
        root,
        &format!("{crate_name}/Cargo.toml"),
        &format!(
            r#"[package]
name = "{crate_name}"
version = "0.0.1"
edition = "2021"
publish = false

[lib]
path = "src/lib.rs"
"#
        ),
    );
    write_fixture_file(root, &format!("{crate_name}/src/lib.rs"), lib_src);
}

fn literals(nodes: &[Node]) -> Vec<&Node> {
    nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::LITERAL)
        .collect()
}

fn prop_str<'a>(props: &'a BTreeMap<String, PropValue>, key: &str) -> &'a str {
    match props
        .get(key)
        .unwrap_or_else(|| panic!("prop {key} missing"))
    {
        PropValue::Str(s) => s.as_str(),
        other => panic!("expected Str for {key}, got {other:?}"),
    }
}

fn prop_int(props: &BTreeMap<String, PropValue>, key: &str) -> i64 {
    match props
        .get(key)
        .unwrap_or_else(|| panic!("prop {key} missing"))
    {
        PropValue::Int(n) => *n,
        other => panic!("expected Int for {key}, got {other:?}"),
    }
}

fn prop_bool(props: &BTreeMap<String, PropValue>, key: &str) -> bool {
    match props
        .get(key)
        .unwrap_or_else(|| panic!("prop {key} missing"))
    {
        PropValue::Bool(b) => *b,
        other => panic!("expected Bool for {key}, got {other:?}"),
    }
}

fn find_literal_by_value<'a>(lits: &'a [&'a Node], value: &str, is_test: Option<bool>) -> &'a Node {
    let candidates: Vec<&&Node> = lits
        .iter()
        .filter(|n| prop_str(&n.props, "value") == value)
        .filter(|n| match is_test {
            Some(want) => prop_bool(&n.props, "is_test") == want,
            None => true,
        })
        .collect();
    match candidates.as_slice() {
        [hit] => hit,
        [] => panic!(
            "no :Literal with value={:?} is_test={:?} in extracted set: {:#?}",
            value,
            is_test,
            lits.iter()
                .map(|n| (
                    prop_str(&n.props, "value"),
                    prop_int(&n.props, "line"),
                    prop_int(&n.props, "col"),
                    prop_bool(&n.props, "is_test"),
                ))
                .collect::<Vec<_>>(),
        ),
        many => panic!(
            "expected exactly one :Literal with value={:?} is_test={:?}, got {} candidates",
            value,
            is_test,
            many.len(),
        ),
    }
}

const LIB_RS: &str = r##"// 1
pub fn case_a_plain_prod_fn() {
    let _ = "verifying";                       // line 3 — case (a)
}

pub fn case_b_raw_string() {
    let _ = r#"shipping"#;                     // line 7 — case (b)
}

pub fn case_c_multiline_escape() {
    let _ = "line1\nline2";                    // line 11 — case (c)
}

#[cfg(test)]
mod tests_mod_d {
    pub fn d_inside_cfg_test_mod() {
        let _ = "case_d_value";                // line 17 — case (d) in-test
    }
}

pub fn case_d_outside_cfg_test_mod() {
    let _ = "case_d_value";                    // line 22 — case (d) prod copy
}

#[test]
fn case_e_inside_hash_test_fn() {
    let _ = "case_e_value";                    // line 27 — case (e)
}

pub const FOO: &str = "constant";              // line 30 — case (f)

pub fn case_g_two_literals_one_fn() {
    let _ = "g_first";                         // line 33 — case (g) first
    let _ = "g_second"; let _ = "g_third";     // line 34 — case (g) second + third (distinct col on same line)
}
"##;

#[test]
fn synthetic_workspace_emits_exact_literal_tuples() {
    let fx = tempdir().expect("tempdir");
    write_cargo_workspace(fx.path(), "lit", LIB_RS);

    let (nodes, _edges) = extract_workspace(fx.path()).expect("extract");
    let lits = literals(&nodes);

    let a = find_literal_by_value(&lits, "verifying", Some(false));
    assert_eq!(prop_str(&a.props, "value"), "verifying");
    assert_eq!(prop_str(&a.props, "file"), "lit/src/lib.rs");
    assert_eq!(prop_int(&a.props, "line"), 3);
    assert_eq!(prop_int(&a.props, "col"), 13);
    assert!(!prop_bool(&a.props, "is_test"));
    assert_eq!(prop_str(&a.props, "crate"), "lit");
    assert_eq!(a.id, "literal:lit/src/lib.rs:3:13");

    let b = find_literal_by_value(&lits, "shipping", Some(false));
    assert_eq!(prop_str(&b.props, "value"), "shipping");
    assert_eq!(prop_str(&b.props, "file"), "lit/src/lib.rs");
    assert_eq!(prop_int(&b.props, "line"), 7);
    assert_eq!(prop_int(&b.props, "col"), 13);
    assert!(!prop_bool(&b.props, "is_test"));

    let c = find_literal_by_value(&lits, "line1\\nline2", Some(false));
    let c_value = prop_str(&c.props, "value");
    assert_eq!(
        c_value.len(),
        12,
        "value MUST be 12 bytes (backslash-n is 2 chars, NOT LF)"
    );
    assert!(
        !c_value.contains('\n'),
        "value MUST NOT contain a real newline — RFC §3.1 forbids LitStr::value() decoding"
    );
    assert!(
        c_value.contains("\\n"),
        "value MUST contain literal backslash-n (2 chars)"
    );
    assert_eq!(prop_int(&c.props, "line"), 11);
    assert!(!prop_bool(&c.props, "is_test"));

    let d_in_test = find_literal_by_value(&lits, "case_d_value", Some(true));
    assert_eq!(prop_int(&d_in_test.props, "line"), 17);
    assert!(prop_bool(&d_in_test.props, "is_test"));

    let d_prod = find_literal_by_value(&lits, "case_d_value", Some(false));
    assert_eq!(prop_int(&d_prod.props, "line"), 22);
    assert!(!prop_bool(&d_prod.props, "is_test"));

    let e = find_literal_by_value(&lits, "case_e_value", Some(true));
    assert_eq!(prop_int(&e.props, "line"), 27);
    assert!(prop_bool(&e.props, "is_test"));

    let f = find_literal_by_value(&lits, "constant", Some(false));
    assert_eq!(prop_int(&f.props, "line"), 30);
    assert!(!prop_bool(&f.props, "is_test"));

    let g1 = find_literal_by_value(&lits, "g_first", Some(false));
    let g2 = find_literal_by_value(&lits, "g_second", Some(false));
    let g3 = find_literal_by_value(&lits, "g_third", Some(false));
    assert_eq!(prop_int(&g1.props, "line"), 33);
    assert_eq!(prop_int(&g2.props, "line"), 34);
    assert_eq!(prop_int(&g3.props, "line"), 34);
    assert_ne!(g1.id, g2.id);
    assert_ne!(g2.id, g3.id);
    assert_ne!(g1.id, g3.id);
    assert!(
        prop_int(&g2.props, "col") < prop_int(&g3.props, "col"),
        "g_second must precede g_third in column order on line 34",
    );

    assert_eq!(
        lits.len(),
        10,
        "expected exactly 10 :Literal nodes (a..g enumerated), got {}: {:#?}",
        lits.len(),
        lits.iter()
            .map(|n| (
                prop_str(&n.props, "value"),
                prop_int(&n.props, "line"),
                prop_int(&n.props, "col"),
                prop_bool(&n.props, "is_test"),
            ))
            .collect::<Vec<_>>(),
    );
}

fn sha256_literals(nodes: &[Node]) -> String {
    let mut lits: Vec<&Node> = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::LITERAL)
        .collect();
    lits.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
    let json = serde_json::to_string(&lits).expect("serialize :Literal subset");
    let mut hasher = Sha256::new();
    hasher.update(json.as_bytes());
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(&mut hex, "{byte:02x}");
    }
    hex
}

#[test]
fn two_sequential_extracts_are_byte_identical_over_literal_set() {
    let fx = tempdir().expect("tempdir");
    write_cargo_workspace(fx.path(), "lit", LIB_RS);

    let (nodes_a, _edges_a) = extract_workspace(fx.path()).expect("run 1");
    let (nodes_b, _edges_b) = extract_workspace(fx.path()).expect("run 2");

    let hash_a = sha256_literals(&nodes_a);
    let hash_b = sha256_literals(&nodes_b);

    assert_eq!(hash_a.len(), 64);
    assert_eq!(hash_b.len(), 64);

    assert_eq!(
        hash_a, hash_b,
        ":Literal extraction is non-deterministic: two runs produced different sha256 hashes \
         (RFC-041 §4 determinism invariant violated)",
    );

    let count_a = literals(&nodes_a).len();
    let count_b = literals(&nodes_b).len();
    assert_eq!(count_a, count_b, ":Literal count drifted between runs");
}
