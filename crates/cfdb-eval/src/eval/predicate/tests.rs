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
