#[cfg(test)]
mod last_segment_tests {
    use std::collections::BTreeMap;

    use cfdb_core::fact::PropValue;
    use cfdb_core::query::{Expr, Param};

    use crate::eval::Evaluator;
    use crate::graph::KeyspaceState;

    /// AC4 — the `last_segment(...)` Cypher UDF MUST delegate to
    /// `cfdb_core::qname::last_segment` (the RFC-035 §3.3 invariant
    /// owner) byte-for-byte. Routing the dispatch through the
    /// canonical helper closes the read-side of the §3.3 invariant
    /// (the write-side closed in slice 3 via `ComputedKey::evaluate`).
    ///
    /// Canary set extends the slice 3 self-dogfood inputs with edge
    /// cases (single-segment, leading/trailing separator, single-`:`
    /// non-qname inputs). The canonical helper splits at the LAST
    /// `::` and returns the trailing segment (or the whole input
    /// when no `::` is present). Pinning these here surfaces any
    /// future divergence between the UDF and the canonical helper
    /// loudly rather than via a downstream Cypher query mismatch.
    #[test]
    fn call_last_segment_agrees_with_canonical_owner_byte_for_byte() {
        let state = KeyspaceState::new();
        let params: BTreeMap<String, Param> = BTreeMap::new();
        let evaluator = Evaluator::new(&state, &params);
        let bindings: BTreeMap<String, crate::eval::Binding> = BTreeMap::new();

        let inputs = [
            "foo::bar::baz",
            "foo",
            "",
            "cfdb_extractor::item_visitor::ItemVisitor::emit_item",
            "single_segment",
            "::leading_separator",
            "trailing_separator::",
            "cfdb_core::qname::last_segment",
        ];

        for input in inputs {
            let expr = Expr::Call {
                name: "last_segment".into(),
                args: vec![Expr::Literal(PropValue::Str(input.to_string()))],
            };
            let actual = evaluator.eval_expr(&expr, &bindings);
            let expected = Some(PropValue::Str(
                cfdb_core::qname::last_segment(input).to_string(),
            ));
            assert_eq!(
                actual, expected,
                "Cypher last_segment UDF diverged from canonical \
                 cfdb_core::qname::last_segment on input {input:?}"
            );
        }
    }

    /// The UDF preserves the `Option<PropValue>` surface — non-string
    /// inputs return `None` (the `?`-on-type-mismatch path shared with
    /// the other UDFs in this dispatcher).
    #[test]
    fn call_last_segment_returns_none_on_non_string_input() {
        let state = KeyspaceState::new();
        let params: BTreeMap<String, Param> = BTreeMap::new();
        let evaluator = Evaluator::new(&state, &params);
        let bindings: BTreeMap<String, crate::eval::Binding> = BTreeMap::new();

        let expr = Expr::Call {
            name: "last_segment".into(),
            args: vec![Expr::Literal(PropValue::Int(42))],
        };
        let actual = evaluator.eval_expr(&expr, &bindings);
        assert_eq!(actual, None);
    }
}
