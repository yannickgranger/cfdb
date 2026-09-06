use crate::emitter::Emitter;
use crate::imports::ImportTable;
use crate::text;

pub(crate) fn buffer_implements_targets(
    clause: tree_sitter::Node,
    src: &[u8],
    current_ns: Option<&str>,
    imports: &ImportTable,
    source_id: &str,
    emitter: &mut Emitter,
) {
    let mut cursor = clause.walk();
    for iface in clause.children(&mut cursor) {
        if matches!(iface.kind(), "name" | "qualified_name") {
            if let Some(target_qname) = resolve_interface_qname(iface, src, current_ns, imports) {
                emitter.buffer_implements(source_id, &target_qname);
            }
        }
    }
}

fn resolve_interface_qname(
    node: tree_sitter::Node,
    src: &[u8],
    current_ns: Option<&str>,
    imports: &ImportTable,
) -> Option<String> {
    let raw = text(node, src)?;
    Some(imports.resolve(raw, current_ns))
}
