//! Slice-7 (#186) — stable rendering of a `NodePattern` into the
//! `(<var>:<Label>)` string used by `cfdb scope --explain`. Lives in a
//! sibling module so `eval/pattern.rs` stays under the 500 LoC god-file
//! ceiling. Dogfood tests grep on the bracket shape — do not change
//! the format without matching updates to their assertions.

use cfdb_core::query::NodePattern;

pub(super) fn format_node_pattern(np: &NodePattern) -> String {
    let var = np.var.as_deref().unwrap_or("");
    let label = np.label.as_ref().map(|l| l.as_str()).unwrap_or("");
    match (var, label) {
        ("", "") => "()".to_string(),
        ("", l) => format!("(:{l})"),
        (v, "") => format!("({v})"),
        (v, l) => format!("({v}:{l})"),
    }
}
