//! Integration tests that build synthetic fixture workspaces in `tempdir`
//! and feed them to `extract_workspace`. Each test exercises a different
//! corner case (macro re-parsing, `#[path]` remap, fn-pointer references,
//! serde default attrs) and asserts on the resulting node/edge set.
//!
//! Moved out of `lib.rs` as part of the #3718 god-module split. These
//! tests never reached into private items — they use only the public
//! `extract_workspace` entry point plus the re-exported `cfdb_core` facts.

use std::path::Path;

use cfdb_core::fact::PropValue;
use cfdb_core::schema::{EdgeLabel, Label};
use cfdb_extractor::extract_workspace;
use tempfile::tempdir;

/// Helper: write a file, creating all parent directories first.
fn write_fixture_file(root: &Path, rel: &str, contents: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(
        p.parent()
            .expect("fixture path always has a parent directory"),
    )
    .expect("fixture mkdir -p succeeds under tempdir");
    std::fs::write(p, contents).expect("fixture write succeeds under tempdir");
}

/// Regression: qbot-core test code wraps call expressions inside
/// `vec![...]`, `assert_eq!(...)`, and similar expression-carrying
/// macros. Before the CallSiteVisitor re-parsed macro tokens, every
/// `Utc::now()` nested inside such a macro was invisible — the
/// blind spot that made walk_forward_tests.rs report 5 CallSites
/// when rg found 21 Utc::now() occurrences.
#[test]
fn call_sites_inside_macro_bodies_are_visible() {
    let fixture = tempdir().expect("tempdir");
    let root = fixture.path();

    write_fixture_file(
        root,
        "Cargo.toml",
        r#"[workspace]
resolver = "2"
members = ["macrofixture"]
"#,
    );
    write_fixture_file(
        root,
        "macrofixture/Cargo.toml",
        r#"[package]
name = "macrofixture"
version = "0.0.1"
edition = "2021"
publish = false

[lib]
path = "src/lib.rs"
"#,
    );
    // Every target call site lives inside a different macro kind
    // to exercise the three parse fallbacks in visit_expr_macro.
    write_fixture_file(
        root,
        "macrofixture/src/lib.rs",
        r#"pub fn fanout() {
    // (1) Punctuated<Expr, Comma> — vec! body.
    let _folds = vec![
        Fold::new(chrono::Utc::now()),
        Fold::new(chrono::Utc::now()),
    ];
    // (2) Punctuated again — assert_eq! pair.
    assert_eq!(
        chrono::Utc::now(),
        chrono::Utc::now()
    );
    // (3) Single-expr fallback — a custom DSL macro wrapping one call.
    wrap!(chrono::Utc::now())
}

pub struct Fold;
impl Fold {
    pub fn new<T>(_t: T) -> Self { Fold }
}

macro_rules! wrap { ($e:expr) => { $e }; }
use wrap;
"#,
    );

    let (nodes, _edges) = extract_workspace(root).expect("extract macro fixture");

    // Count Utc::now call sites tagged to the `fanout` caller —
    // syn text reading should see 5 of them:
    //   vec![Fold::new(Utc::now()), Fold::new(Utc::now())] = 2
    //   assert_eq!(Utc::now(), Utc::now())                 = 2
    //   wrap!(Utc::now())                                  = 1
    let utc_calls = nodes
        .iter()
        .filter(|n| {
            n.label.as_str() == Label::CALL_SITE
                && n.props
                    .get("caller_qname")
                    .and_then(PropValue::as_str)
                    .map(|s| s.ends_with("::fanout"))
                    .unwrap_or(false)
                && n.props
                    .get("callee_path")
                    .and_then(PropValue::as_str)
                    .map(|s| s.contains("Utc::now"))
                    .unwrap_or(false)
        })
        .count();
    assert_eq!(
        utc_calls, 5,
        "expected 5 Utc::now CallSites inside fanout's macro bodies, got {utc_calls}; \
         the re-parse fallback probably regressed"
    );
}

/// Regression: qbot-core uses `#[path = "x_tests.rs"]` + `#[cfg(test)]`
/// heavily to attach test modules to their subject files (309 instances
/// in the real tree, 236 of them pointing at test files). The extractor
/// must (a) resolve the override to the actual file, and (b) propagate
/// the `is_test` flag across the file boundary so items inside the
/// child file are tagged correctly. Both were broken before this test
/// landed.
///
/// **AC: `is_test_attribute_extracted_from_cfg_test`** (issue #3727 /
/// council-cfdb-wiring §B.1.1). This test cross-validates that the
/// extractor tags items under a `#[cfg(test)]` module with `is_test=true`
/// — which is the other half of AC-1 alongside the bare `#[test]`
/// fixture in [`is_test_attribute_extracted_from_hash_test`].
#[test]
fn path_attribute_remap_preserves_test_flag() {
    let fixture = tempdir().expect("tempdir");
    let root = fixture.path();

    // Workspace + one lib crate.
    write_fixture_file(
        root,
        "Cargo.toml",
        r#"[workspace]
resolver = "2"
members = ["pathfixture"]
"#,
    );
    write_fixture_file(
        root,
        "pathfixture/Cargo.toml",
        r#"[package]
name = "pathfixture"
version = "0.0.1"
edition = "2021"
publish = false

[lib]
path = "src/lib.rs"
"#,
    );

    // lib.rs declares a file module that in turn has a `#[path]`-
    // remapped test module. Matches qbot-core's stable pattern:
    //   mod subject;          // src/subject.rs (prod)
    //   #[cfg(test)]
    //   #[path = "subject_tests.rs"]
    //   mod tests;            // src/subject_tests.rs (test)
    //
    // The test file is a sibling of `subject.rs` in `src/`, NOT in
    // `src/subject/`. rustc resolves `#[path]` relative to `parent`
    // of the declaring file; our resolver tries both.
    write_fixture_file(
        root,
        "pathfixture/src/lib.rs",
        r#"pub mod subject;
"#,
    );
    write_fixture_file(
        root,
        "pathfixture/src/subject.rs",
        r#"pub fn prod_fn() -> i64 {
    1
}

#[cfg(test)]
#[path = "subject_tests.rs"]
mod tests;
"#,
    );
    write_fixture_file(
        root,
        "pathfixture/src/subject_tests.rs",
        r#"#[test]
fn test_fn_that_calls_utc_now() {
    let _cheat = chrono::Utc::now();
}
"#,
    );

    let (nodes, _edges) = extract_workspace(root).expect("extract path fixture");

    // 1. The file node for the remapped test file must be present.
    let has_test_file = nodes.iter().any(|n| {
        n.label.as_str() == Label::FILE
            && n.props
                .get("path")
                .and_then(PropValue::as_str)
                .map(|s| s.contains("subject_tests.rs"))
                .unwrap_or(false)
    });
    assert!(
        has_test_file,
        "expected File node for src/subject_tests.rs; the #[path] attr was not honored"
    );

    // 2. The Item `test_fn_that_calls_utc_now` must exist AND be
    //    tagged `is_test=true`. Before this fix it was either missing
    //    entirely (path override not honored) or present but tagged
    //    `is_test=false` (flag didn't propagate).
    let test_fn = nodes.iter().find(|n| {
        n.label.as_str() == Label::ITEM
            && n.props.get("name").and_then(PropValue::as_str) == Some("test_fn_that_calls_utc_now")
    });
    let test_fn = test_fn.expect("test_fn_that_calls_utc_now Item missing");
    assert_eq!(
        test_fn.props.get("is_test").and_then(PropValue::as_bool),
        Some(true),
        "test fn must be tagged is_test=true"
    );

    // 3. The prod fn in the same file must exist and be is_test=false.
    let prod_fn = nodes
        .iter()
        .find(|n| {
            n.label.as_str() == Label::ITEM
                && n.props.get("name").and_then(PropValue::as_str) == Some("prod_fn")
        })
        .expect("prod_fn Item missing");
    assert_eq!(
        prod_fn.props.get("is_test").and_then(PropValue::as_bool),
        Some(false),
        "prod fn must be tagged is_test=false"
    );

    // 4. The CallSite for chrono::Utc::now() inside the test file
    //    must exist and be tagged is_test=true — this is the bug that
    //    was hiding hundreds of hits on real qbot-core.
    let utc_cs = nodes.iter().find(|n| {
        n.label.as_str() == Label::CALL_SITE
            && n.props
                .get("callee_path")
                .and_then(PropValue::as_str)
                .map(|s| s.contains("Utc::now"))
                .unwrap_or(false)
    });
    let utc_cs = utc_cs.expect("Utc::now CallSite missing from remapped test file");
    assert_eq!(
        utc_cs.props.get("is_test").and_then(PropValue::as_bool),
        Some(true),
        "test-file CallSite must inherit is_test=true across the file boundary"
    );
}

/// Regression for QA-5 spike (issue #3623): `.unwrap_or_else(Utc::now)`
/// and similar fn-pointer-passing idioms were invisible to the v0.1
/// CallSiteVisitor because `Utc::now` as an `ExprPath` argument is not
/// an `ExprCall`. On the real qbot-core tree this hid 13 prod ban
/// violations. The fix: iterate `ExprCall::args` and `ExprMethodCall::args`
/// and emit a `kind="fn_ptr"` CallSite for each `Expr::Path` found.
#[test]
fn fn_pointer_path_refs_emit_call_sites() {
    let fixture = tempdir().expect("tempdir");
    let root = fixture.path();
    write_fixture_file(
        root,
        "Cargo.toml",
        r#"[workspace]
resolver = "2"
members = ["fnptrfixture"]
"#,
    );
    write_fixture_file(
        root,
        "fnptrfixture/Cargo.toml",
        r#"[package]
name = "fnptrfixture"
version = "0.0.1"
edition = "2021"
publish = false

[lib]
path = "src/lib.rs"
"#,
    );
    write_fixture_file(
        root,
        "fnptrfixture/src/lib.rs",
        r#"pub fn method_call_fn_ptr(opt: Option<i64>) -> i64 {
    // Method-call shape: .unwrap_or_else(Utc::now).timestamp() —
    // Utc::now is an ExprPath argument to a method call.
    opt.unwrap_or_else(chrono::Utc::now).timestamp()
}

pub fn free_fn_call_fn_ptr() {
    // Free-function call shape: bar(Foo::build) — Foo::build is an
    // ExprPath passed as a positional argument.
    run(chrono::Utc::now);
}

pub fn run<F: Fn() -> chrono::DateTime<chrono::Utc>>(_f: F) {}
"#,
    );

    let (nodes, _) = extract_workspace(root).expect("extract fn-ptr fixture");

    // Both fn-pointer references must surface as CallSite nodes with
    // `kind="fn_ptr"` and `callee_path` ending in `Utc::now`.
    let fn_ptrs: Vec<_> = nodes
        .iter()
        .filter(|n| {
            n.label.as_str() == Label::CALL_SITE
                && n.props.get("kind").and_then(PropValue::as_str) == Some("fn_ptr")
                && n.props
                    .get("callee_path")
                    .and_then(PropValue::as_str)
                    .map(|s| s.ends_with("Utc::now"))
                    .unwrap_or(false)
        })
        .collect();
    assert_eq!(
        fn_ptrs.len(),
        2,
        "expected 2 fn_ptr CallSites for Utc::now (method-call arg + free-fn arg); got {}",
        fn_ptrs.len()
    );

    // Direct `.timestamp()` method call must STILL be emitted as a
    // regular method CallSite — the fn-ptr addition must not replace
    // existing emission.
    let has_timestamp_method = nodes.iter().any(|n| {
        n.label.as_str() == Label::CALL_SITE
            && n.props
                .get("callee_path")
                .and_then(PropValue::as_str)
                .map(|s| s == "timestamp")
                .unwrap_or(false)
    });
    assert!(
        has_timestamp_method,
        "existing method-call emission regressed — `timestamp` should still appear"
    );
}

/// Regression for QA-5 spike (issue #3623): `#[serde(default = "Utc::now")]`
/// on a struct field is a name-based reference to a callable that is
/// invoked at deserialization time. Not an `ExprCall`, not in any fn
/// body, so CallSiteVisitor would miss it. The fix: scan field attrs
/// in `visit_item_struct` and emit a `kind="serde_default"` CallSite
/// linked from the owning struct Item.
#[test]
fn serde_default_attribute_emits_call_site() {
    let fixture = tempdir().expect("tempdir");
    let root = fixture.path();
    write_fixture_file(
        root,
        "Cargo.toml",
        r#"[workspace]
resolver = "2"
members = ["serdefixture"]
"#,
    );
    write_fixture_file(
        root,
        "serdefixture/Cargo.toml",
        r#"[package]
name = "serdefixture"
version = "0.0.1"
edition = "2021"
publish = false

[lib]
path = "src/lib.rs"
"#,
    );
    write_fixture_file(
        root,
        "serdefixture/src/lib.rs",
        r#"// Synthetic — this crate never compiles for real. The extractor
// only uses syn to parse, so missing type `DateTime<Utc>` is OK.
pub struct Tick {
    #[serde(default = "Utc::now")]
    pub received_at: DateTime<Utc>,
    pub price: f64,
}
"#,
    );

    let (nodes, edges) = extract_workspace(root).expect("extract serde fixture");

    // The CallSite must exist with kind="serde_default" and
    // callee_path="Utc::now".
    let serde_cs: Vec<_> = nodes
        .iter()
        .filter(|n| {
            n.label.as_str() == Label::CALL_SITE
                && n.props.get("kind").and_then(PropValue::as_str) == Some("serde_default")
                && n.props.get("callee_path").and_then(PropValue::as_str) == Some("Utc::now")
        })
        .collect();
    assert_eq!(
        serde_cs.len(),
        1,
        "expected 1 serde_default CallSite for Utc::now on `received_at`; got {}",
        serde_cs.len()
    );

    // And the INVOKES_AT edge must flow from the owning struct Item.
    let cs_id = &serde_cs[0].id;
    let has_invokes_edge = edges.iter().any(|e| {
        e.label.as_str() == EdgeLabel::INVOKES_AT && e.dst == *cs_id && e.src.contains("::Tick")
    });
    assert!(
        has_invokes_edge,
        "expected INVOKES_AT edge from Tick struct Item → serde_default CallSite, got edges: {:?}",
        edges.iter().filter(|e| e.dst == *cs_id).collect::<Vec<_>>()
    );
}

/// AC: `is_test_attribute_extracted_from_hash_test` (issue #3727 /
/// council-cfdb-wiring §B.1.1). A free `fn` marked with bare `#[test]`
/// outside any `#[cfg(test)]` module must be tagged `is_test=true`, and
/// a sibling `fn` with no attribute must stay `is_test=false`. This was
/// a gap in the pre-existing path: `attrs_contain_cfg_test` handled
/// `#[cfg(test)]` on modules but never the `#[test]` marker on free fns.
#[test]
fn is_test_attribute_extracted_from_hash_test() {
    let fixture = tempdir().expect("tempdir");
    let root = fixture.path();

    write_fixture_file(
        root,
        "Cargo.toml",
        r#"[workspace]
resolver = "2"
members = ["hashtestfixture"]
"#,
    );
    write_fixture_file(
        root,
        "hashtestfixture/Cargo.toml",
        r#"[package]
name = "hashtestfixture"
version = "0.0.0"
edition = "2021"
"#,
    );
    // Note: NO `#[cfg(test)]` module wrapping the `#[test]` fn. This is
    // the scenario the existing `attrs_contain_cfg_test` path misses.
    write_fixture_file(
        root,
        "hashtestfixture/src/lib.rs",
        r#"
pub fn prod_fn() -> i32 { 1 }

#[test]
fn bare_hash_test_fn() {
    assert_eq!(prod_fn(), 1);
}
"#,
    );

    let (nodes, _edges) = extract_workspace(root).expect("extract_workspace");

    let bare = nodes
        .iter()
        .find(|n| {
            n.label.as_str() == Label::ITEM
                && n.props.get("name").and_then(PropValue::as_str) == Some("bare_hash_test_fn")
        })
        .expect("bare_hash_test_fn item missing");
    assert_eq!(
        bare.props.get("is_test").and_then(PropValue::as_bool),
        Some(true),
        "`#[test] fn bare_hash_test_fn()` must be tagged is_test=true"
    );

    let prod = nodes
        .iter()
        .find(|n| {
            n.label.as_str() == Label::ITEM
                && n.props.get("name").and_then(PropValue::as_str) == Some("prod_fn")
        })
        .expect("prod_fn item missing");
    assert_eq!(
        prod.props.get("is_test").and_then(PropValue::as_bool),
        Some(false),
        "prod_fn with no attribute must stay is_test=false"
    );
}

/// AC: `bounded_context_derived_from_crate_prefix` — end-to-end check
/// that the crate-prefix heuristic stamps `Item.bounded_context` on
/// every item in a crate whose name starts with a well-known prefix.
#[test]
fn bounded_context_derived_from_crate_prefix() {
    let fixture = tempdir().expect("tempdir");
    let root = fixture.path();

    write_fixture_file(
        root,
        "Cargo.toml",
        r#"[workspace]
resolver = "2"
members = ["domain-trading", "ports-trading"]
"#,
    );
    write_fixture_file(
        root,
        "domain-trading/Cargo.toml",
        r#"[package]
name = "domain-trading"
version = "0.0.0"
edition = "2021"
"#,
    );
    write_fixture_file(root, "domain-trading/src/lib.rs", r#"pub struct Position;"#);
    write_fixture_file(
        root,
        "ports-trading/Cargo.toml",
        r#"[package]
name = "ports-trading"
version = "0.0.0"
edition = "2021"
"#,
    );
    write_fixture_file(
        root,
        "ports-trading/src/lib.rs",
        r#"pub trait OrderRouter {}"#,
    );

    let (nodes, edges) = extract_workspace(root).expect("extract_workspace");

    let position = nodes
        .iter()
        .find(|n| {
            n.label.as_str() == Label::ITEM
                && n.props.get("name").and_then(PropValue::as_str) == Some("Position")
        })
        .expect("Position item missing");
    assert_eq!(
        position
            .props
            .get("bounded_context")
            .and_then(PropValue::as_str),
        Some("trading"),
        "domain-trading::Position should resolve to bounded_context=`trading`"
    );

    let order_router = nodes
        .iter()
        .find(|n| {
            n.label.as_str() == Label::ITEM
                && n.props.get("name").and_then(PropValue::as_str) == Some("OrderRouter")
        })
        .expect("OrderRouter item missing");
    assert_eq!(
        order_router
            .props
            .get("bounded_context")
            .and_then(PropValue::as_str),
        Some("trading"),
        "ports-trading::OrderRouter should also resolve to bounded_context=`trading`"
    );

    // AC: `context_node_emitted_for_each_declared_context`
    // A single `:Context` node named `trading` must exist for both crates.
    let trading_context = nodes
        .iter()
        .filter(|n| {
            n.label.as_str() == Label::CONTEXT
                && n.props.get("name").and_then(PropValue::as_str) == Some("trading")
        })
        .count();
    assert_eq!(
        trading_context, 1,
        "expected exactly one :Context node named `trading`"
    );

    // AC: `belongs_to_edge_connects_crate_to_context`
    // Both crates must have one BELONGS_TO edge to the `trading` context.
    for crate_name in ["domain-trading", "ports-trading"] {
        let src = format!("crate:{crate_name}");
        let belongs: Vec<_> = edges
            .iter()
            .filter(|e| e.label.as_str() == EdgeLabel::BELONGS_TO && e.src == src)
            .collect();
        assert_eq!(
            belongs.len(),
            1,
            "{crate_name} must have exactly one BELONGS_TO edge"
        );
        assert_eq!(
            belongs[0].dst, "context:trading",
            "{crate_name} BELONGS_TO must target context:trading"
        );
    }
}

/// AC: `bounded_context_overridden_by_concepts_toml` — an override file
/// at `.cfdb/concepts/*.toml` beats the crate-prefix heuristic.
#[test]
fn bounded_context_overridden_by_concepts_toml() {
    let fixture = tempdir().expect("tempdir");
    let root = fixture.path();

    // Workspace + one crate whose name would heuristically resolve to `lonely`
    // (from `domain-lonely`), but the override remaps it to `portfolio`.
    write_fixture_file(
        root,
        "Cargo.toml",
        r#"[workspace]
resolver = "2"
members = ["domain-lonely"]
"#,
    );
    write_fixture_file(
        root,
        "domain-lonely/Cargo.toml",
        r#"[package]
name = "domain-lonely"
version = "0.0.0"
edition = "2021"
"#,
    );
    write_fixture_file(root, "domain-lonely/src/lib.rs", r#"pub struct Lonely;"#);
    // Override: map `domain-lonely` into the `portfolio` context.
    write_fixture_file(
        root,
        ".cfdb/concepts/portfolio.toml",
        r#"
name = "portfolio"
crates = ["domain-lonely"]
canonical_crate = "domain-portfolio"
owning_rfc = "RFC-007"
"#,
    );

    let (nodes, edges) = extract_workspace(root).expect("extract_workspace");

    let lonely = nodes
        .iter()
        .find(|n| {
            n.label.as_str() == Label::ITEM
                && n.props.get("name").and_then(PropValue::as_str) == Some("Lonely")
        })
        .expect("Lonely item missing");
    assert_eq!(
        lonely
            .props
            .get("bounded_context")
            .and_then(PropValue::as_str),
        Some("portfolio"),
        "override must win over heuristic — domain-lonely should map to `portfolio`, not `lonely`"
    );

    // The :Context node must carry canonical_crate + owning_rfc from the override.
    let portfolio = nodes
        .iter()
        .find(|n| {
            n.label.as_str() == Label::CONTEXT
                && n.props.get("name").and_then(PropValue::as_str) == Some("portfolio")
        })
        .expect(":Context{name=portfolio} missing");
    assert_eq!(
        portfolio
            .props
            .get("canonical_crate")
            .and_then(PropValue::as_str),
        Some("domain-portfolio"),
        "canonical_crate from override must land on the :Context node"
    );
    assert_eq!(
        portfolio
            .props
            .get("owning_rfc")
            .and_then(PropValue::as_str),
        Some("RFC-007"),
        "owning_rfc from override must land on the :Context node"
    );

    // And the single BELONGS_TO edge from `crate:domain-lonely` targets `context:portfolio`.
    let belongs: Vec<_> = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::BELONGS_TO && e.src == "crate:domain-lonely")
        .collect();
    assert_eq!(belongs.len(), 1);
    assert_eq!(belongs[0].dst, "context:portfolio");
}

/// SchemaVersion v0.1.3+ — every `:CallSite` node carries the
/// `resolver` and `callee_resolved` discriminator properties (issue #83,
/// RFC-029 §A1.2 homonym mitigation). The syn-based extractor ALWAYS
/// emits `resolver="syn"` + `callee_resolved=false`; never any other
/// value. This fixture exercises all four call-site kinds that emit
/// `:CallSite` (`call`, `fn_ptr`, `method`, `serde_default`) in one
/// workspace so the assertion covers both emit_call_site and
/// emit_attr_call_site paths with a single extraction.
#[test]
fn every_syn_call_site_carries_resolver_and_callee_resolved_discriminators() {
    let fixture = tempdir().expect("tempdir");
    let root = fixture.path();

    write_fixture_file(
        root,
        "Cargo.toml",
        r#"[workspace]
resolver = "2"
members = ["discfixture"]
"#,
    );
    write_fixture_file(
        root,
        "discfixture/Cargo.toml",
        r#"[package]
name = "discfixture"
version = "0.0.1"
edition = "2021"
"#,
    );
    // All four CallSite kinds exercised in one crate:
    //   - `call`          (ExprCall)
    //   - `fn_ptr`        (path-as-arg to a fn-pointer parameter)
    //   - `method`        (MethodCall)
    //   - `serde_default` (#[serde(default = "…")] on a struct field)
    write_fixture_file(
        root,
        "discfixture/src/lib.rs",
        r#"
pub fn greet() -> String { String::from("hi") }

pub fn register(_f: fn() -> String) {}

pub struct Counter(pub u32);
impl Counter {
    pub fn tick(&mut self) { self.0 += 1; }
}

pub fn default_answer() -> u32 { 42 }

#[derive(Debug)]
pub struct Config {
    #[serde(default = "default_answer")]
    pub answer: u32,
}

pub fn demo() {
    let _ = greet();           // call
    register(greet);           // fn_ptr
    let mut c = Counter(0);
    c.tick();                  // method
}
"#,
    );

    let (nodes, _edges) = extract_workspace(root).expect("extract discfixture");

    let call_sites: Vec<_> = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::CALL_SITE)
        .collect();

    // Guard: the fixture must actually produce :CallSite nodes — else
    // the discriminator assertion below vacuously passes.
    assert!(
        call_sites.len() >= 3,
        "discfixture should emit ≥3 :CallSite nodes; got {}",
        call_sites.len(),
    );

    // Per-kind coverage — both emission paths are exercised:
    //   * emit_call_site (call_visitor.rs) → `call`, `fn_ptr`, `method`
    //   * emit_attr_call_site (item_visitor.rs) → `serde_default`
    let observed_kinds: std::collections::BTreeSet<_> = call_sites
        .iter()
        .filter_map(|n| n.props.get("kind").and_then(PropValue::as_str))
        .map(str::to_string)
        .collect();
    for expected_kind in ["call", "fn_ptr", "method", "serde_default"] {
        assert!(
            observed_kinds.contains(expected_kind),
            "fixture must emit a :CallSite of kind={expected_kind} to exercise the discriminator \
             on both emit paths; observed kinds: {observed_kinds:?}",
        );
    }

    // The core assertion: every emitted :CallSite carries the v0.1.3
    // discriminator properties — `resolver="syn"` and
    // `callee_resolved=false`. No syn-extracted :CallSite ever claims
    // `resolver="hir"` or `callee_resolved=true`; those values are
    // reserved for cfdb-hir-extractor (v0.2+).
    for cs in &call_sites {
        let id = &cs.id;
        let resolver = cs
            .props
            .get("resolver")
            .and_then(PropValue::as_str)
            .unwrap_or_else(|| {
                panic!("{id}: :CallSite missing `resolver` prop (v0.1.3+ contract)")
            });
        assert_eq!(
            resolver, "syn",
            "{id}: cfdb-extractor must emit `resolver=\"syn\"`, got {resolver:?}"
        );
        let callee_resolved = cs.props.get("callee_resolved").unwrap_or_else(|| {
            panic!("{id}: :CallSite missing `callee_resolved` prop (v0.1.3+ contract)")
        });
        match callee_resolved {
            PropValue::Bool(false) => {}
            PropValue::Bool(true) => {
                panic!(
                    "{id}: syn-based extractor must never emit `callee_resolved=true`; \
                     that value is reserved for cfdb-hir-extractor (v0.2+)"
                );
            }
            other => panic!("{id}: `callee_resolved` must be a Bool prop, got {other:?}",),
        }
    }
}
