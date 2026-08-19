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

    let (db, vfs, _pm_client, targets) =
        build_hir_database(root, false).expect("build_hir_database on hirfixture for line scar");
    let (nodes, _edges) = extract_call_sites(&db, &vfs, root, &targets)
        .expect("extract_call_sites on hirfixture for line scar");

    let hir_call_sites: Vec<_> = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::CALL_SITE)
        .filter(|n| n.props.get("resolver").and_then(PropValue::as_str) == Some("hir"))
        .collect();
    assert!(
        !hir_call_sites.is_empty(),
        "fixture produced zero HIR :CallSite nodes — extraction broken upstream of this scar"
    );

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
    use ra_ap_ide_db::line_index::LineIndex;
    use ra_ap_syntax::TextSize;

    let text = "fn a() {}\nfn b() {}\n";
    let idx = LineIndex::new(text);
    let line = idx.line_col(TextSize::from(0)).line as usize + 1;
    assert_eq!(line, 1);
    let line2 = idx.line_col(TextSize::from(10)).line as usize + 1;
    assert_eq!(line2, 2);
}
