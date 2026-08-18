#[cfg(test)]
mod last_segment_tests {
    use std::collections::BTreeMap;

    use cfdb_core::fact::PropValue;
    use cfdb_core::graph::{GraphBackend, GraphReader};
    use cfdb_core::query::{Expr, ParamBinding};
    use cfdb_core::schema::Keyspace;
    use cfdb_core::store::StoreBackend;
    use cfdb_petgraph::PetgraphStore;

    use crate::eval::Evaluator;

    /// An empty keyspace, read through the port — the UDFs under test
    /// never touch the graph.
    fn empty_store() -> (PetgraphStore, Keyspace) {
        let ks = Keyspace::new("udf");
        let mut store = PetgraphStore::new();
        store
            .ingest_nodes(&ks, Vec::new())
            .expect("create keyspace");
        (store, ks)
    }

    fn reader((store, ks): &(PetgraphStore, Keyspace)) -> &dyn GraphReader {
        store.graph_reader(ks).expect("known keyspace")
    }

    /// The `last_segment(...)` Cypher UDF MUST delegate to
    /// `cfdb_core::qname::last_segment` byte-for-byte. Routing the
    /// dispatch through the canonical helper closes the read-side of
    /// the invariant. The canonical helper splits at the LAST `::` and
    /// returns the trailing segment (or the whole input when no `::` is
    /// present). Pinning these here surfaces any future divergence
    /// between the UDF and the canonical helper loudly rather than via
    /// a downstream Cypher query mismatch.
    #[test]
    fn call_last_segment_agrees_with_canonical_owner_byte_for_byte() {
        let state = empty_store();
        let params: BTreeMap<String, ParamBinding> = BTreeMap::new();
        let evaluator = Evaluator::new(reader(&state), &params);
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
        let state = empty_store();
        let params: BTreeMap<String, ParamBinding> = BTreeMap::new();
        let evaluator = Evaluator::new(reader(&state), &params);
        let bindings: BTreeMap<String, crate::eval::Binding> = BTreeMap::new();

        let expr = Expr::Call {
            name: "last_segment".into(),
            args: vec![Expr::Literal(PropValue::Int(42))],
        };
        let actual = evaluator.eval_expr(&expr, &bindings);
        assert_eq!(actual, None);
    }
}
