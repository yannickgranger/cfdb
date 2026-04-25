//! F-005 / EPIC #273 regression scar — `:CallSite.line` from the
//! HIR-based extractor must be a real 1-indexed source-line number,
//! not the `0` placeholder shipped by the original RFC-029 §A1.2
//! slice (Issue #85c).
//!
//! ## Why this scar exists
//!
//! Before this fix, [`extract_call_sites`] always emitted
//! `line: PropValue::Int(0)` for every resolved method call. PR #291
//! had already fixed the parallel issue in the syn-based extractor
//! (`cfdb-extractor`); the HIR path was missed. The resulting
//! cross-extractor inconsistency meant the same `foo.bar()` resolved
//! by both extractors produced two `:CallSite` nodes — one with
//! `resolver="syn"` and a real line, one with `resolver="hir"` and
//! line=0 — defeating any line-precision query that joined across
//! resolvers.
//!
//! ## What this scar asserts
//!
//! Two complementary assertions on a synthetic single-crate fixture:
//!
//! 1. **Concrete line.** The fixture places `g.greet()` on a known
//!    line (line 8 in the embedded `lib.rs`). The emitted
//!    `:CallSite.line` for that call MUST equal 8. Off-by-one in
//!    either direction (0-indexed leak, or BOM/header miscount)
//!    fails this.
//! 2. **Coverage threshold.** Across every HIR-resolved `:CallSite`
//!    node the fixture produces, ≥ 50% must carry `line > 0`. The
//!    fixture is small enough that 100% is the realistic outcome;
//!    the 50% lower bound matches the convention F-005 set in
//!    `cfdb-extractor/tests/self_workspace.rs` so a future
//!    regression where `LineIndex` silently no-ops still trips the
//!    scar.
//!
//! Cross-extractor consistency (the same call resolved by both syn
//! and HIR producing the same line) is documented as a follow-up
//! gap — the syn extractor isn't reachable from this crate's tests
//! (architectural boundary), so the assertion lives in PR-body
//! prose for now.

use std::fs;
use std::path::Path;

use cfdb_core::fact::PropValue;
use cfdb_core::schema::Label;
use cfdb_hir_extractor::{build_hir_database, extract_call_sites};
use tempfile::tempdir;

fn write(root: &Path, rel: &str, contents: &str) {
    let p = root.join(rel);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).expect("fixture mkdir -p");
    }
    fs::write(p, contents).expect("fixture write");
}

/// Fixture body. Line numbering is 1-indexed from the first byte of
/// the stored file.
///
/// File layout (1-indexed):
///   1: pub struct Greeter;
///   2: (blank)
///   3: impl Greeter {
///   4:     pub fn greet(&self) -> &'static str { "hello" }
///   5: }
///   6: pub fn dispatch() -> &'static str {
///   7:     let g = Greeter;
///   8:     g.greet()
///   9: }
const FIXTURE_LIB_RS: &str = "pub struct Greeter;

impl Greeter {
    pub fn greet(&self) -> &'static str { \"hello\" }
}
pub fn dispatch() -> &'static str {
    let g = Greeter;
    g.greet()
}
";

const EXPECTED_GREET_CALL_LINE: i64 = 8;

#[test]
fn test_f005_hir_callsite_line_is_real_not_zero() {
    let tmp = tempdir().expect("tempdir");
    let root = tmp.path();

    write(
        root,
        "Cargo.toml",
        r#"[workspace]
resolver = "2"
members = ["hirfixture"]
"#,
    );
    write(
        root,
        "hirfixture/Cargo.toml",
        r#"[package]
name = "hirfixture"
version = "0.0.1"
edition = "2021"

[dependencies]
"#,
    );
    write(root, "hirfixture/src/lib.rs", FIXTURE_LIB_RS);

    let (db, vfs) =
        build_hir_database(root).expect("build_hir_database on hirfixture for line scar");
    let (nodes, _edges) =
        extract_call_sites(&db, &vfs).expect("extract_call_sites on hirfixture for line scar");

    let hir_call_sites: Vec<_> = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::CALL_SITE)
        .filter(|n| n.props.get("resolver").and_then(PropValue::as_str) == Some("hir"))
        .collect();
    assert!(
        !hir_call_sites.is_empty(),
        "fixture produced zero HIR :CallSite nodes — extraction broken upstream of this scar"
    );

    // (1) Concrete-line assertion on the `g.greet()` call.
    let greet_call_site = hir_call_sites
        .iter()
        .find(|n| {
            n.props
                .get("callee_path")
                .and_then(PropValue::as_str)
                .is_some_and(|p| p.ends_with("Greeter::greet"))
        })
        .expect("expected a HIR :CallSite for Greeter::greet — fixture or resolution regression");

    let actual_line = greet_call_site
        .props
        .get("line")
        .and_then(PropValue::as_i64)
        .expect(":CallSite.line must be an Int prop");
    assert_eq!(
        actual_line, EXPECTED_GREET_CALL_LINE,
        "F-005 / #273 regression: HIR :CallSite for `g.greet()` reported line={actual_line}, \
         expected line={EXPECTED_GREET_CALL_LINE} (the line the fixture's `g.greet()` sits on). \
         If this is 0, the hardcoded `PropValue::Int(0)` in `emit_resolved_call` came back. \
         If this is off by one, check the 0-indexed → 1-indexed conversion in `walk_file`."
    );

    // (2) Coverage threshold — ≥ 50% of HIR :CallSite nodes carry line > 0.
    let total = hir_call_sites.len();
    let with_real_line = hir_call_sites
        .iter()
        .filter(|n| {
            n.props
                .get("line")
                .and_then(PropValue::as_i64)
                .is_some_and(|l| l > 0)
        })
        .count();
    let percentage = (with_real_line * 100) / total;
    assert!(
        percentage >= 50,
        "F-005 / #273 regression: only {with_real_line} of {total} HIR :CallSite nodes \
         ({percentage}%) carry line>0 — expected >= 50%. If this drops below 50% the \
         `LineIndex`-driven offset → line conversion is silently returning 0."
    );
}

#[test]
fn test_f005_line_at_offset_zero_is_line_one() {
    // Pure boundary-condition guard for the 0-indexed → 1-indexed
    // shift in `walk_file`. The first byte of a non-empty source file
    // is on line 1, never line 0. This test does not call into the
    // HIR extractor — it only proves we know what the LineIndex API
    // returns at offset 0, so a future refactor that drops the `+ 1`
    // adjustment trips a clear local failure rather than a confusing
    // mid-file off-by-one.
    use ra_ap_ide_db::line_index::LineIndex;
    use ra_ap_syntax::TextSize;

    let text = "fn a() {}\nfn b() {}\n";
    let idx = LineIndex::new(text);
    // 0-indexed line at byte 0 is 0; +1 makes it 1.
    let line = idx.line_col(TextSize::from(0)).line as usize + 1;
    assert_eq!(line, 1);
    // After the first newline (byte 10 = "fn a() {}\n".len()), we are on line 2.
    let line2 = idx.line_col(TextSize::from(10)).line as usize + 1;
    assert_eq!(line2, 2);
}
