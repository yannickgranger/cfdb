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
//!
//! Per-label node-id formulas are in the [`node_id`] submodule and
//! re-exported here so callers of `cfdb_core::qname::field_node_id`
//! (etc.) keep working without modification.

mod node_id;
pub use node_id::{field_node_id, item_node_id, param_node_id, variant_node_id};

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

/// Canonicalise an `impl` target rendering by dropping any generic
/// argument list. Both the syn-based `cfdb-extractor` and the
/// HIR-based `cfdb-hir-extractor` feed their raw type rendering
/// through this function before calling [`method_qname`], so an
/// `impl Vec<Node> { fn push }` produces the same qname via either
/// extractor (`<crate>::Vec::push`, not `<crate>::Vec<Node>::push`
/// from HIR and `<crate>::Vec::push` from syn).
///
/// Without this normalisation, syn's shallow renderer
/// (`cfdb-extractor/src/type_render.rs::render_type`) produces
/// `Vec` — which strips the generic args — while HIR's
/// `HirDisplay::display` produces `Vec<Node>`. That divergence
/// would make cross-extractor `CALLS(item:…, item:…)` edges dangle
/// silently — exactly the failure mode the #40 ddd-specialist review
/// flagged as HIGH and the #94 review caught as still unremediated.
///
/// Algorithm: strip every character at bracket depth ≥ 1 (where
/// depth tracks nested `<` / `>`). Trailing whitespace that remains
/// after a stripped region is trimmed.
#[must_use]
pub fn normalize_impl_target(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut depth: usize = 0;
    for c in raw.chars() {
        match c {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            _ if depth == 0 => out.push(c),
            _ => {}
        }
    }
    out.truncate(out.trim_end().len());
    out
}

/// Trailing segment of a `::`-delimited qname — splits at the **last**
/// `::` and returns the portion after it. Inputs containing no `::`
/// (a degenerate-but-valid qname carrying just an item name) are
/// returned unchanged.
///
/// Canonical owner of the `last_segment` formula for the entire
/// workspace (RFC-035 §3.3 / R1 B3 — `cfdb-core::qname` is cfdb's
/// invariant owner for qname structure). Every consumer that needs a
/// `last_segment` value MUST route through this function — including
/// the `:Item` index-build dispatch in `cfdb-petgraph::index` (called
/// via [`ComputedKey::evaluate`](../../../cfdb_petgraph/index/spec/enum.ComputedKey.html#method.evaluate),
/// not directly), and any future consumer.
///
/// Round-trips with the qname constructors in this module:
/// `last_segment(item_qname(stack, name)) == name`,
/// `last_segment(method_qname(stack, target, method)) == method`. The
/// `qname_contract_sync` test module asserts this property mechanically
/// on a sampled set of stacks so a future change to either
/// [`module_qpath`] or this function fails the build instead of
/// silently drifting.
///
/// Pure: zero allocations, returns a borrowed slice into the input.
#[must_use]
pub fn last_segment(qname: &str) -> &str {
    qname.rsplit_once("::").map_or(qname, |(_, tail)| tail)
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
    fn normalize_impl_target_strips_single_generic_param() {
        assert_eq!(normalize_impl_target("Vec<Node>"), "Vec");
    }

    #[test]
    fn normalize_impl_target_strips_nested_generics() {
        assert_eq!(normalize_impl_target("Vec<HashMap<K, V>>"), "Vec");
        assert_eq!(
            normalize_impl_target("BTreeMap<String, Vec<u8>>"),
            "BTreeMap"
        );
    }

    #[test]
    fn normalize_impl_target_preserves_qualified_paths_without_generics() {
        assert_eq!(
            normalize_impl_target("std::fmt::Display"),
            "std::fmt::Display"
        );
        assert_eq!(normalize_impl_target("Foo"), "Foo");
    }

    #[test]
    fn normalize_impl_target_strips_lifetime_and_bounds() {
        assert_eq!(normalize_impl_target("Foo<'a, T: Bound>"), "Foo");
        assert_eq!(normalize_impl_target("Iter<'a, T>"), "Iter");
    }

    #[test]
    fn normalize_impl_target_trims_trailing_whitespace_after_strip() {
        assert_eq!(normalize_impl_target("Foo <T>"), "Foo");
    }

    #[test]
    fn normalize_impl_target_preserves_non_angle_punctuation() {
        assert_eq!(normalize_impl_target("&dyn Foo"), "&dyn Foo");
        assert_eq!(normalize_impl_target("(A, B)"), "(A, B)");
    }

    #[test]
    fn normalize_impl_target_handles_empty_input() {
        assert_eq!(normalize_impl_target(""), "");
    }

    #[test]
    fn normalize_impl_target_handles_unmatched_closing_bracket() {
        // Defensive: saturating_sub prevents underflow on malformed
        // input. We don't claim correctness on garbage — just no panic.
        assert_eq!(normalize_impl_target("Foo>"), "Foo");
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

    #[test]
    fn last_segment_returns_trailing_segment_after_double_colon() {
        assert_eq!(last_segment("foo::bar::baz"), "baz");
    }

    #[test]
    fn last_segment_returns_input_unchanged_when_no_separator() {
        assert_eq!(last_segment("foo"), "foo");
    }

    #[test]
    fn last_segment_handles_empty_input() {
        assert_eq!(last_segment(""), "");
    }
}

/// Mechanical guarantor of the qname-contract-drift invariant
/// (RFC-035 §3.3 invariant 2 / §5.2 solid-architect NIT).
///
/// `last_segment` is the inverse of the trailing-name-append operation
/// performed by [`item_qname`] and [`method_qname`]. If either side of
/// that pairing changes shape — for example, if [`module_qpath`] starts
/// emitting a different separator, or [`item_qname`] alters how it
/// joins the trailing name — these assertions fail at build time and
/// catch the drift before any downstream consumer (the index-build
/// dispatch in `cfdb-petgraph::index`, the Cypher `last_segment()`
/// UDF, the extractor's `:CallSite.callee_last_segment` prop) sees
/// silently divergent values.
///
/// The fixtures mirror the `module_qpath` / `item_qname` /
/// `method_qname` test stacks so a future update to those tests
/// naturally extends here.
#[cfg(test)]
mod qname_contract_sync {
    use super::*;

    fn stack(elements: &[&str]) -> Vec<String> {
        elements.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn last_segment_recovers_item_name_from_item_qname() {
        let cases: &[(&[&str], &str)] = &[
            (&["cfdb_core", "schema", "labels"], "Label"),
            (&["cfdb_cli", "commands"], "bind_json_params"),
            (&["cfdb_extractor"], "ItemVisitor"),
            (&["cfdb_petgraph", "index", "spec"], "ComputedKey"),
        ];
        for (s, name) in cases {
            let q = item_qname(&stack(s), name);
            assert_eq!(
                last_segment(&q),
                *name,
                "drift: item_qname({s:?}, {name:?}) → {q:?} but last_segment recovered {:?}",
                last_segment(&q)
            );
        }
    }

    #[test]
    fn last_segment_recovers_method_name_from_method_qname() {
        let cases: &[(&[&str], &str, &str)] = &[
            (&["cfdb_cli", "commands"], "Store", "open"),
            (
                &["cfdb_extractor", "item_visitor"],
                "ItemVisitor",
                "emit_item",
            ),
            (&["cfdb_core", "fact"], "Vec<Node>", "push"),
            (
                &["cfdb_petgraph", "index", "spec"],
                "ComputedKey",
                "evaluate",
            ),
        ];
        for (s, target, method) in cases {
            let q = method_qname(&stack(s), target, method);
            assert_eq!(
                last_segment(&q),
                *method,
                "drift: method_qname({s:?}, {target:?}, {method:?}) → {q:?} but last_segment recovered {:?}",
                last_segment(&q)
            );
        }
    }

    #[test]
    fn last_segment_consistent_with_module_qpath_then_append() {
        // last_segment(module_qpath(stack) + "::" + name) == name — the
        // exact wording of the RFC §5.2 NIT.
        let cases: &[(&[&str], &str)] = &[
            (&["cfdb_core", "schema", "labels"], "Label"),
            (&["cfdb_cli"], "bind_json_params"),
        ];
        for (s, name) in cases {
            let qpath = module_qpath(&stack(s));
            let q = format!("{qpath}::{name}");
            assert_eq!(last_segment(&q), *name);
        }
    }

    #[test]
    fn last_segment_recovers_trailing_token_after_node_id_strip() {
        // Round-trip: build a node id, strip the prefix, then ask for
        // the last segment — the stripping is independent of the
        // splitter, but the whole composition is what production code
        // does (`qname_from_node_id` followed by `last_segment`).
        let q = item_qname(&stack(&["cfdb_extractor", "item_visitor"]), "ItemVisitor");
        let node_id = item_node_id(&q);
        let bare = qname_from_node_id(&node_id);
        assert_eq!(last_segment(bare), "ItemVisitor");
    }
}
