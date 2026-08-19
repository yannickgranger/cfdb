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
