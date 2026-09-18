use std::collections::BTreeMap;

use cfdb_core::fact::{Edge, Node};
use cfdb_core::qname::global_read_node_id;
use cfdb_core::schema::{EdgeLabel, Label};

use crate::emitter::{item_id, Emitter};
use crate::text;

const SUPERGLOBALS: [&str; 9] = [
    "GLOBALS", "_SERVER", "_GET", "_POST", "_FILES", "_COOKIE", "_SESSION", "_REQUEST", "_ENV",
];

pub(crate) fn visit_variable_name(
    node: tree_sitter::Node,
    src: &[u8],
    caller_qname: &str,
    file: &str,
    counts: &mut BTreeMap<String, usize>,
    emitter: &mut Emitter,
) {
    let Some(name) = variable_name_text(node, src) else {
        return;
    };
    if !SUPERGLOBALS.contains(&name) {
        return;
    }

    let key = format!("global:{name}");
    let idx = {
        let counter = counts.entry(key).or_insert(0);
        let i = *counter;
        *counter += 1;
        i
    };
    let id = global_read_node_id(caller_qname, name, idx);

    emitter.emit_node(
        Node::new(id.as_str(), Label::new(Label::GLOBAL_READ))
            .with_prop("name", name)
            .with_prop("caller_qname", caller_qname)
            .with_prop("file", file)
            .with_prop("line", (node.start_position().row + 1) as i64),
    );
    emitter.emit_edge(Edge::new(
        item_id(caller_qname),
        id,
        EdgeLabel::new(EdgeLabel::READS_GLOBAL),
    ));
}

fn variable_name_text<'a>(node: tree_sitter::Node, src: &'a [u8]) -> Option<&'a str> {
    let mut cursor = node.walk();
    let name = node.children(&mut cursor).find(|c| c.kind() == "name")?;
    text(name, src)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_superglobal_the_rfc_names_is_in_the_closed_set() {
        for name in [
            "GLOBALS", "_SERVER", "_GET", "_POST", "_FILES", "_COOKIE", "_SESSION", "_REQUEST",
            "_ENV",
        ] {
            assert!(
                SUPERGLOBALS.contains(&name),
                "{name} is one of the nine PHP superglobals cfdb-062-php-declared-shapes#3.7 names"
            );
        }
        assert_eq!(
            SUPERGLOBALS.len(),
            9,
            "the set is exactly the nine PHP superglobals, no more"
        );
    }

    #[test]
    fn an_ordinary_variable_is_not_a_superglobal() {
        assert!(!SUPERGLOBALS.contains(&"env"));
        assert!(!SUPERGLOBALS.contains(&"this"));
    }
}
