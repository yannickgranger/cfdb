//! `:Literal` synthetic-workspace integration fixture — **THE correctness
//! gate for RFC-041 `:Literal` extraction** (RFC-041 §4 invariants /
//! issue cfdb#371 / slice 041-C).
//!
//! Per RFC-041 §4: `:Literal` is **out of the recall corpus by
//! construction** (no `rustdoc-json` / `cargo public-api` oracle exists
//! for expression-leaf literals — CLAUDE.md §5's "RFC adding a new fact
//! type MUST extend the recall corpus" clause is inapplicable). Slice
//! 041-B's self-dogfood `count ≥ N` assertion is a smoke test only — it
//! cannot catch an extraction bug that yields the right count via wrong
//! literals. **This file is the ratified substitute correctness gate**:
//! exact `(value, file, line, col, is_test)` tuple assertions against a
//! synthetic Cargo workspace covering RFC-041 §7's seven enumerated
//! cases (a)-(g) plus a sha256 byte-identical-across-runs determinism
//! subtest.
//!
//! ## Conventions mirrored
//!
//! Layout and helper signatures mirror `const_table_emission.rs`
//! (`write_fixture_file` / `write_cargo_workspace` / `prop_str` /
//! `prop_int` / `prop_bool` at lines 19-106) — same synthetic-workspace
//! pattern, same `extract_workspace(fx.path())` invocation, same
//! `PropValue` destructure helpers. Determinism subtest mirrors
//! `self_workspace.rs::extractor_is_deterministic_across_two_runs`
//! (lines 274-301) but scopes the comparison to the `:Literal` subset
//! and additionally compares sha256 hex (per RFC §7 041-C bullet:
//! "two sequential extracts byte-identical (sha256 of serialized
//! `:Literal` set)").

use std::collections::BTreeMap;
use std::path::Path;

use cfdb_core::fact::{Node, PropValue};
use cfdb_core::schema::Label;
use cfdb_extractor::extract_workspace;
use sha2::{Digest, Sha256};
use tempfile::tempdir;

// ---------------------------------------------------------------------------
// Fixture-writing helpers — verbatim from `const_table_emission.rs:19-52`.
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Node + prop accessors — verbatim from `const_table_emission.rs:54-106`.
// ---------------------------------------------------------------------------

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

/// Find a `:Literal` node by `value` (and an optional `is_test`
/// disambiguator for case (d) — same value text appears twice with
/// different `is_test`). Panics with a diagnostic listing all extracted
/// literals if no match is found.
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

// ---------------------------------------------------------------------------
// The seven cases (a)-(g) in one synthetic workspace.
// ---------------------------------------------------------------------------
//
// Each case is encoded as a single chunk of Rust source written into
// `lit/src/lib.rs`. Cases are concatenated into one file so the test
// asserts the FULL `:Literal` set the extractor produces from a single
// `extract_workspace` call (matches RFC §7 041-C bullet language: "the
// exact set of (value, file, line, col, is_test) tuples").
//
// **Line offsets matter** — assertions below pin `line` for every
// case. Adding / removing a blank line shifts everything. The fixture
// source is structured so the relevant literal sits at a fixed,
// commented `// line N` marker.

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

// ---------------------------------------------------------------------------
// (a)-(g) tuple assertions.
// ---------------------------------------------------------------------------

#[test]
fn synthetic_workspace_emits_exact_literal_tuples() {
    let fx = tempdir().expect("tempdir");
    write_cargo_workspace(fx.path(), "lit", LIB_RS);

    let (nodes, _edges) = extract_workspace(fx.path()).expect("extract");
    let lits = literals(&nodes);

    // Every assertion below is a hard tuple match. We do NOT assert
    // `lits.len() == 8` first — that would be a count assertion which
    // §4 explicitly says is smoke-only. Instead, every case (a)-(g)
    // asserts the precise `(value, file, line, col, is_test)` tuple,
    // and at the end we assert the count to catch stray emissions.

    // -- case (a) plain "verifying" in a production fn → is_test=false ----
    let a = find_literal_by_value(&lits, "verifying", Some(false));
    assert_eq!(prop_str(&a.props, "value"), "verifying");
    assert_eq!(prop_str(&a.props, "file"), "lit/src/lib.rs");
    assert_eq!(prop_int(&a.props, "line"), 3);
    // Col is 1-indexed start of the literal. `    let _ = "verifying";`
    // → 4 spaces + `let _ = ` (8 chars) + opening quote = column 13.
    assert_eq!(prop_int(&a.props, "col"), 13);
    assert!(!prop_bool(&a.props, "is_test"));
    assert_eq!(prop_str(&a.props, "crate"), "lit");
    // Node id contract (RFC §3.1): `literal:<file>:<line>:<col>`.
    assert_eq!(a.id, "literal:lit/src/lib.rs:3:13");

    // -- case (b) raw string r#"shipping"# → value="shipping" (no r/#) ----
    // RFC §3.1 "Value normalization": raw strings store INNER bytes,
    // without the `r` / `#` delimiters and without escape decoding.
    let b = find_literal_by_value(&lits, "shipping", Some(false));
    assert_eq!(prop_str(&b.props, "value"), "shipping");
    assert_eq!(prop_str(&b.props, "file"), "lit/src/lib.rs");
    assert_eq!(prop_int(&b.props, "line"), 7);
    // `    let _ = r#"shipping"#;` — `(line, col)` is the start of the
    // `r` prefix per proc-macro2's `Span::start()`. 4 spaces +
    // `let _ = ` (8 chars) + `r` = column 13. The RFC §3.1 contract
    // pins `value` to the inner bytes regardless of the position
    // convention, but the position convention itself is implementer's
    // choice; we record proc-macro2's choice here.
    assert_eq!(prop_int(&b.props, "col"), 13);
    assert!(!prop_bool(&b.props, "is_test"));

    // -- case (c) "line1\nline2" — value has LITERAL backslash-n -----------
    // RFC §3.1 invariant: `value` stores source bytes between quotes,
    // NOT `LitStr::value()`'s decoded form. So `\n` (2 chars) NOT LF
    // (1 char). This is the load-bearing case for the `=~`-matches-
    // grep invariant: `grep '\\n'` matches what `value` contains.
    //
    // Bytes asserted explicitly so JSON/Rust string escaping ambiguity
    // can't sneak in: the value MUST be 12 bytes — l, i, n, e, 1,
    // backslash (1 byte), n, l, i, n, e, 2.
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

    // -- case (d) same literal text inside AND outside `#[cfg(test)] mod` --
    // RFC §7 041-C bullet (d) demands BOTH be emitted with the right
    // flags: the in-test copy with is_test=true, the prod copy with
    // is_test=false.
    let d_in_test = find_literal_by_value(&lits, "case_d_value", Some(true));
    assert_eq!(prop_int(&d_in_test.props, "line"), 17);
    assert!(prop_bool(&d_in_test.props, "is_test"));

    let d_prod = find_literal_by_value(&lits, "case_d_value", Some(false));
    assert_eq!(prop_int(&d_prod.props, "line"), 22);
    assert!(!prop_bool(&d_prod.props, "is_test"));

    // -- case (e) literal inside `#[test] fn` → is_test=true ---------------
    let e = find_literal_by_value(&lits, "case_e_value", Some(true));
    assert_eq!(prop_int(&e.props, "line"), 27);
    assert!(prop_bool(&e.props, "is_test"));

    // -- case (f) `pub const FOO: &str = "constant";` → is_test=false -------
    let f = find_literal_by_value(&lits, "constant", Some(false));
    assert_eq!(prop_int(&f.props, "line"), 30);
    assert!(!prop_bool(&f.props, "is_test"));

    // -- case (g) THREE literals in one fn body, distinct (line, col) ------
    // RFC §7 041-C bullet (g) requires "two literals on different
    // lines/cols in one fn → both emitted, distinct coords". The
    // fixture goes slightly further to also exercise distinct-col-
    // same-line (g_second + g_third both on line 34): the node ID
    // contract `literal:<file>:<line>:<col>` is collision-free only if
    // col differs.
    let g1 = find_literal_by_value(&lits, "g_first", Some(false));
    let g2 = find_literal_by_value(&lits, "g_second", Some(false));
    let g3 = find_literal_by_value(&lits, "g_third", Some(false));
    assert_eq!(prop_int(&g1.props, "line"), 33);
    assert_eq!(prop_int(&g2.props, "line"), 34);
    assert_eq!(prop_int(&g3.props, "line"), 34);
    // Distinct coords means distinct ids (RFC §3.1 collision-freeness).
    assert_ne!(g1.id, g2.id);
    assert_ne!(g2.id, g3.id);
    assert_ne!(g1.id, g3.id);
    // Same line but distinct cols: g2.col < g3.col strictly.
    assert!(
        prop_int(&g2.props, "col") < prop_int(&g3.props, "col"),
        "g_second must precede g_third in column order on line 34",
    );

    // -- Final cardinality: exactly 9 :Literal nodes in this fixture -------
    //   (a) verifying, (b) shipping, (c) line1\nline2,
    //   (d-test) case_d_value, (d-prod) case_d_value,
    //   (e) case_e_value, (f) constant,
    //   (g) g_first + g_second + g_third = 3
    //   total = 1+1+1+2+1+1+3 = 10
    //
    // JUDGMENT CALL: the RFC §6 Non-goals explicitly EXCLUDE attribute-
    // embedded strings (`#[doc=...]`), `cfg(feature="...")` strings,
    // `#[serde(default = "...")]` strings, and `macro_rules!` body
    // literals. The fixture has none of these so the count is the
    // sum of intended emissions only — no exclusion-rule literal
    // leaks expected. If the producer over-emits (e.g., picks up a
    // literal inside the workspace `[package] name = "lit"` TOML —
    // which it must not, since TOML is not Rust source), this
    // assertion catches it.
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

// ---------------------------------------------------------------------------
// Determinism — two sequential extracts produce byte-identical
// serialized `:Literal` set (sha256 hex equal).
// ---------------------------------------------------------------------------

/// Hash the serialized form of the `:Literal` subset of `nodes`.
///
/// Nodes are pre-sorted by their canonical sort key so the comparison
/// is robust to any future iteration-order drift in `extract_workspace`
/// (RFC §3.2: "nodes emitted in source traversal order, sorted by
/// (file, line, col) before serialization"). The sort here is a
/// belt-and-braces — if the producer is correct the input is already
/// sorted; if it is not, the hash equality still holds across two runs
/// as long as the non-determinism is stable per-process (which is
/// itself a bug worth catching by separate assertion).
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

    // sha256 hex is 64 chars — sanity check the helper itself.
    assert_eq!(hash_a.len(), 64);
    assert_eq!(hash_b.len(), 64);

    assert_eq!(
        hash_a, hash_b,
        ":Literal extraction is non-deterministic: two runs produced different sha256 hashes \
         (RFC-041 §4 determinism invariant violated)",
    );

    // Belt-and-braces: also compare structurally so a hash collision
    // (vanishingly unlikely but stylistically clean to also assert) is
    // caught.
    let count_a = literals(&nodes_a).len();
    let count_b = literals(&nodes_b).len();
    assert_eq!(count_a, count_b, ":Literal count drifted between runs");
}
