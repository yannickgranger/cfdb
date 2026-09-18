use cfdb_core::fact::{Edge, Node};
use cfdb_core::qname::supertype_node_id;
use cfdb_core::schema::{EdgeLabel, Label};

use crate::emitter::{item_id, Emitter};
use crate::imports::ImportTable;
use crate::text;

pub(crate) fn emit_supertypes(
    class_like: tree_sitter::Node,
    src: &[u8],
    current_ns: Option<&str>,
    imports: &ImportTable,
    qname: &str,
    file: &str,
    emitter: &mut Emitter,
) {
    let owner_id = item_id(qname);
    let mut idx = 0usize;
    idx = emit_clause(
        class_like,
        src,
        current_ns,
        imports,
        qname,
        &owner_id,
        file,
        "base_clause",
        "extends",
        idx,
        emitter,
    );
    emit_clause(
        class_like,
        src,
        current_ns,
        imports,
        qname,
        &owner_id,
        file,
        "class_interface_clause",
        "implements",
        idx,
        emitter,
    );
}

#[allow(clippy::too_many_arguments)]
fn emit_clause(
    class_like: tree_sitter::Node,
    src: &[u8],
    current_ns: Option<&str>,
    imports: &ImportTable,
    qname: &str,
    owner_id: &str,
    file: &str,
    clause_kind: &str,
    relation: &str,
    mut idx: usize,
    emitter: &mut Emitter,
) -> usize {
    let mut cursor = class_like.walk();
    for clause in class_like.children(&mut cursor) {
        if clause.kind() != clause_kind {
            continue;
        }
        let mut name_cursor = clause.walk();
        for name_node in clause.children(&mut name_cursor) {
            if !matches!(name_node.kind(), "name" | "qualified_name") {
                continue;
            }
            let Some(written) = text(name_node, src) else {
                continue;
            };
            let fqn = imports.resolve(written, current_ns);
            let line = (name_node.start_position().row + 1) as i64;
            let id = supertype_node_id(qname, idx);
            emitter.emit_node(
                Node::new(&id, Label::new(Label::SUPERTYPE))
                    .with_prop("relation", relation)
                    .with_prop("written", written)
                    .with_prop("fqn", fqn.as_str())
                    .with_prop("file", file)
                    .with_prop("line", line),
            );
            emitter.emit_edge(Edge::new(
                owner_id,
                &id,
                EdgeLabel::new(EdgeLabel::HAS_SUPERTYPE),
            ));
            if relation == "extends" {
                emitter.buffer_extends(owner_id, &fqn);
            }
            idx += 1;
        }
    }
    idx
}
