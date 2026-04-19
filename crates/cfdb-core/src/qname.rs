//! Canonical derivation of item qnames — the stable cross-extractor
//! ID contract for `cfdb`'s fact graph.
//!
//! Both the syn-based `cfdb-extractor` and the HIR-based
//! `cfdb-hir-extractor` (v0.2+ — Issue #85c) emit `Item` nodes with
//! IDs of the form `item:<qname>` and reference those IDs from other
//! facts (e.g., `CALLS(item:caller, item:callee)`). For cross-extractor
//! edges to land on the same `:Item` node, both extractors MUST
//! compute the `<qname>` component bit-identically for the same source
//! item. Any formula divergence produces silently dangling edges — the
//! worst class of graph corruption because it passes every schema
//! validator while making every reachability query wrong.
//!
//! The functions in this module are the single canonical formula.
//! `cfdb-extractor` (syn) uses them via direct call. `cfdb-hir-extractor`
//! will use them after projecting HIR types down to the same
//! `(module_stack, item_name)` tuple.
//!
//! All functions are pure: values in → values out, zero I/O, zero
//! allocations beyond the return `String`.

/// Join the module stack into a `::`-delimited module qpath.
///
/// The module stack convention (matching `cfdb-extractor/src/item_visitor.rs`):
/// the first element is the crate name with dashes replaced by underscores
/// (e.g. `cfdb_core`), followed by nested `mod` names from the crate root
/// to the current visit position.
///
/// An empty stack yields an empty string. A single-element stack yields
/// just that element (no trailing `::`).
#[must_use]
pub fn module_qpath(module_stack: &[String]) -> String {
    module_stack.join("::")
}

/// Qname for a non-method item (struct, enum, fn, const, trait, impl,
/// type-alias, static, module). Takes the enclosing module stack and the
/// item's unqualified name; produces `<module_qpath>::<item_name>`, or
/// just `<item_name>` when the stack is empty.
///
/// The empty-stack branch is a degenerate fallback — in practice the
/// stack always contains at least the crate name.
#[must_use]
pub fn item_qname(module_stack: &[String], item_name: &str) -> String {
    let qpath = module_qpath(module_stack);
    if qpath.is_empty() {
        item_name.to_string()
    } else {
        format!("{qpath}::{item_name}")
    }
}

/// Qname for a method inside an `impl Target { fn method }` block.
/// Produces `<module_qpath>::<impl_target>::<method_name>`. The impl
/// target is the textual rendering of `self_ty` (e.g., `Foo`,
/// `Foo<T>`, `Vec<T>`).
///
/// When the module stack is empty, the `<module_qpath>::` prefix is
/// elided so the result is `<impl_target>::<method_name>`.
#[must_use]
pub fn method_qname(module_stack: &[String], impl_target: &str, method_name: &str) -> String {
    let qpath = module_qpath(module_stack);
    if qpath.is_empty() {
        format!("{impl_target}::{method_name}")
    } else {
        format!("{qpath}::{impl_target}::{method_name}")
    }
}

/// Wrap a qname into the canonical `:Item` node-id form
/// (`item:<qname>`). This prefix is the graph-level convention used
/// by every edge whose source or target is an Item.
#[must_use]
pub fn item_node_id(qname: &str) -> String {
    format!("item:{qname}")
}

/// Inverse of [`item_node_id`] — strip the `item:` prefix off a node
/// id to recover the bare qname. Callers sometimes round-trip an Item
/// node id back to its qname to pass through a helper that expects
/// bare qnames (e.g. emitter functions that receive a qname and
/// internally re-wrap via `item_node_id`). Routing both directions
/// through this module prevents the prefix literal from re-scattering
/// into hand-written `trim_start_matches("item:")` calls.
///
/// If the input does not carry the `item:` prefix, it is returned
/// unchanged — symmetric with `str::trim_start_matches` behaviour.
#[must_use]
pub fn qname_from_node_id(node_id: &str) -> &str {
    node_id.strip_prefix("item:").unwrap_or(node_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: concise construction of a module stack from a &[&str].
    fn stack(elements: &[&str]) -> Vec<String> {
        elements.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn module_qpath_joins_with_double_colon() {
        assert_eq!(
            module_qpath(&stack(&["cfdb_core", "schema", "labels"])),
            "cfdb_core::schema::labels"
        );
    }

    #[test]
    fn module_qpath_empty_stack_is_empty_string() {
        assert_eq!(module_qpath(&stack(&[])), "");
    }

    #[test]
    fn module_qpath_single_element_has_no_trailing_separator() {
        assert_eq!(module_qpath(&stack(&["cfdb_cli"])), "cfdb_cli");
    }

    #[test]
    fn item_qname_appends_name_under_module() {
        assert_eq!(
            item_qname(&stack(&["cfdb_core", "schema"]), "Label"),
            "cfdb_core::schema::Label"
        );
    }

    #[test]
    fn item_qname_with_crate_root_stack_produces_crate_prefix() {
        assert_eq!(
            item_qname(&stack(&["cfdb_cli"]), "bind_json_params"),
            "cfdb_cli::bind_json_params"
        );
    }

    #[test]
    fn item_qname_with_empty_stack_falls_back_to_bare_name() {
        assert_eq!(item_qname(&stack(&[]), "Thing"), "Thing");
    }

    #[test]
    fn method_qname_interposes_impl_target_between_module_and_method() {
        assert_eq!(
            method_qname(&stack(&["cfdb_cli", "commands"]), "Store", "open"),
            "cfdb_cli::commands::Store::open"
        );
    }

    #[test]
    fn method_qname_with_generic_impl_target_preserves_angle_brackets() {
        assert_eq!(
            method_qname(&stack(&["cfdb_core", "fact"]), "Vec<Node>", "push"),
            "cfdb_core::fact::Vec<Node>::push"
        );
    }

    #[test]
    fn method_qname_with_empty_module_stack_drops_leading_separator() {
        assert_eq!(method_qname(&stack(&[]), "Foo", "bar"), "Foo::bar");
    }

    #[test]
    fn method_qname_single_crate_element_stack() {
        assert_eq!(
            method_qname(&stack(&["cfdb_extractor"]), "ItemVisitor", "qname"),
            "cfdb_extractor::ItemVisitor::qname"
        );
    }

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
    fn qname_from_node_id_strips_item_prefix() {
        assert_eq!(
            qname_from_node_id("item:cfdb_core::schema::Label"),
            "cfdb_core::schema::Label"
        );
    }

    #[test]
    fn qname_from_node_id_returns_input_unchanged_when_no_prefix() {
        assert_eq!(qname_from_node_id("no_prefix_here"), "no_prefix_here");
    }

    #[test]
    fn qname_from_node_id_round_trip_via_item_node_id() {
        let q = "cfdb_extractor::item_visitor::ItemVisitor::emit_item";
        assert_eq!(qname_from_node_id(&item_node_id(q)), q);
    }

    #[test]
    fn method_qname_with_qualified_impl_target_preserves_path() {
        // `impl std::fmt::Display for Foo` — the rendered self_ty is
        // the trait target `Foo`, but nothing in the formula prevents
        // a caller from passing a qualified path. The contract is
        // verbatim interposition between module_qpath and method_name.
        assert_eq!(
            method_qname(&stack(&["cfdb_cli"]), "std::fmt::Display", "fmt"),
            "cfdb_cli::std::fmt::Display::fmt"
        );
    }

    #[test]
    fn item_qname_then_node_id_composes() {
        let q = item_qname(&stack(&["cfdb_core", "schema", "labels"]), "Label");
        assert_eq!(item_node_id(&q), "item:cfdb_core::schema::labels::Label");
    }

    #[test]
    fn method_qname_then_node_id_composes() {
        let q = method_qname(
            &stack(&["cfdb_extractor", "item_visitor"]),
            "ItemVisitor",
            "emit_item",
        );
        assert_eq!(
            item_node_id(&q),
            "item:cfdb_extractor::item_visitor::ItemVisitor::emit_item"
        );
    }
}
