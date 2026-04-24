//! Canonical per-label node-id formulas for `cfdb`'s fact graph.
//!
//! Each function produces the stable node-id for one label kind. Both
//! the syn-based `cfdb-extractor` and the HIR-based
//! `cfdb-hir-extractor` route through these helpers so cross-extractor
//! edges always target the same node id (RFC-037 §3.8 B8 canonical id
//! helpers).
//!
//! All functions are pure: values in → values out, zero I/O, zero
//! allocations beyond the return `String`.

/// Wrap a qname into the canonical `:Item` node-id form
/// (`item:<qname>`). This prefix is the graph-level convention used
/// by every edge whose source or target is an Item.
#[must_use]
pub fn item_node_id(qname: &str) -> String {
    format!("item:{qname}")
}

/// Canonical `:Param` node-id form (#209, RFC-036 §3.1 CP1).
///
/// Parameters of the same fn are disambiguated by positional index,
/// never by name — a fn can legitimately have two params named `_`
/// (wildcard patterns) and must still get distinct node ids.
/// Extractors (syn-based today, HIR-based tomorrow) must route
/// through this function so cross-extractor `HAS_PARAM` /
/// `REGISTERS_PARAM` edges target the same node id.
#[must_use]
pub fn param_node_id(parent_qname: &str, index: usize) -> String {
    format!("param:{parent_qname}#{index}")
}

/// Canonical node id for a `:Field`. Both extractors (syn-based today,
/// HIR-based tomorrow) route through this function so cross-extractor
/// `HAS_FIELD` / `REGISTERS_PARAM` edges target the same node id.
/// Mirrors the #209 resolution of the `:Param` id split-brain for
/// `:Field`.
#[must_use]
pub fn field_node_id(parent_qname: &str, field_name: &str) -> String {
    format!("field:{parent_qname}.{field_name}")
}

/// Canonical node id for a `:Variant`. Index-based to mirror
/// `param_node_id` — positionally stable within a single extract;
/// variant reordering produces a new id (delete + recreate in diffs),
/// accepted per the same tradeoff as `param_node_id`.
#[must_use]
pub fn variant_node_id(enum_qname: &str, index: usize) -> String {
    format!("variant:{enum_qname}#{index}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_node_id_prefixes_with_item_colon() {
        assert_eq!(
            item_node_id("cfdb_core::schema::Label"),
            "item:cfdb_core::schema::Label"
        );
    }

    #[test]
    fn item_node_id_is_string_prefix_not_structural() {
        // Degenerate but valid — the wrapper is a pure string operation,
        // not a parser. Empty qname is illegal-but-representable.
        assert_eq!(item_node_id(""), "item:");
    }

    #[test]
    fn param_node_id_disambiguates_by_index_not_name() {
        // A fn with two wildcard params must produce two distinct
        // node ids — the index is load-bearing, name alone is not.
        let parent = "crate::module::fn_name";
        assert_eq!(param_node_id(parent, 0), "param:crate::module::fn_name#0");
        assert_eq!(param_node_id(parent, 1), "param:crate::module::fn_name#1");
        assert_ne!(param_node_id(parent, 0), param_node_id(parent, 1));
    }

    #[test]
    fn field_node_id_formula_is_field_colon_parent_dot_name() {
        assert_eq!(field_node_id("crate::Foo", "bar"), "field:crate::Foo.bar");
    }

    #[test]
    fn field_node_id_handles_tuple_field_name_convention() {
        // Tuple fields use "_0", "_1", ... by convention (RFC-037 §3.3).
        assert_eq!(field_node_id("crate::Foo", "_0"), "field:crate::Foo._0");
    }

    #[test]
    fn variant_node_id_formula_is_variant_colon_parent_hash_index() {
        assert_eq!(variant_node_id("crate::E", 0), "variant:crate::E#0");
    }

    #[test]
    fn variant_node_id_disambiguates_by_index_not_name() {
        // Two variants with the same position-0 index but different names
        // would collide under a name-based scheme. Index-based avoids it.
        let a = variant_node_id("crate::E", 0);
        let b = variant_node_id("crate::E", 1);
        assert_ne!(a, b);
    }
}
