use std::collections::BTreeMap;

use crate::{qualify, text};

#[derive(Debug, Default)]
pub(crate) struct ImportTable {
    aliases: BTreeMap<String, String>,
}

impl ImportTable {
    pub(crate) fn resolve(&self, raw: &str, current_ns: Option<&str>) -> String {
        if let Some(absolute) = raw.strip_prefix('\\') {
            return absolute.to_string();
        }
        let (head, rest) = match raw.split_once('\\') {
            Some((head, rest)) => (head, Some(rest)),
            None => (raw, None),
        };
        match self.aliases.get(&head.to_ascii_lowercase()) {
            Some(fqn) => match rest {
                Some(rest) => format!("{fqn}\\{rest}"),
                None => fqn.clone(),
            },
            None => qualify(current_ns, raw),
        }
    }

    fn insert(&mut self, alias: &str, fqn: String) {
        self.aliases.insert(alias.to_ascii_lowercase(), fqn);
    }
}

pub(crate) fn collect(program: tree_sitter::Node, src: &[u8]) -> ImportTable {
    let mut table = ImportTable::default();
    let mut cursor = program.walk();
    for child in program.children(&mut cursor) {
        if child.kind() == "namespace_use_declaration" {
            absorb_declaration(child, src, &mut table);
        }
    }
    table
}

fn imports_a_symbol_not_a_class(clause: tree_sitter::Node, src: &[u8]) -> bool {
    let mut cursor = clause.walk();
    let children: Vec<tree_sitter::Node> = clause.children(&mut cursor).collect();
    children
        .iter()
        .any(|child| !child.is_named() && matches!(text(*child, src), Some("function" | "const")))
}

fn absorb_declaration(decl: tree_sitter::Node, src: &[u8], table: &mut ImportTable) {
    let mut cursor = decl.walk();
    let children: Vec<tree_sitter::Node> = decl.children(&mut cursor).collect();
    let group = children.iter().find(|c| c.kind() == "namespace_use_group");
    let Some(group) = group else {
        for clause in children
            .iter()
            .filter(|c| c.kind() == "namespace_use_clause")
        {
            absorb_clause(*clause, src, None, table);
        }
        return;
    };
    let prefix = children
        .iter()
        .find(|c| c.kind() == "namespace_name")
        .and_then(|n| text(*n, src));
    let mut group_cursor = group.walk();
    for clause in group
        .children(&mut group_cursor)
        .filter(|c| c.kind() == "namespace_use_clause")
    {
        absorb_clause(clause, src, prefix, table);
    }
}

fn absorb_clause(
    clause: tree_sitter::Node,
    src: &[u8],
    prefix: Option<&str>,
    table: &mut ImportTable,
) {
    if imports_a_symbol_not_a_class(clause, src) {
        return;
    }
    let alias = clause
        .child_by_field_name("alias")
        .and_then(|a| text(a, src));
    let mut cursor = clause.walk();
    let Some(path_node) = clause
        .children(&mut cursor)
        .find(|c| matches!(c.kind(), "qualified_name" | "name"))
    else {
        return;
    };
    let Some(path) = text(path_node, src) else {
        return;
    };
    let path = path.trim_start_matches('\\');
    let fqn = match prefix {
        Some(prefix) => format!("{prefix}\\{path}"),
        None => path.to_string(),
    };
    let last = fqn.rsplit('\\').next().unwrap_or(fqn.as_str()).to_string();
    table.insert(alias.unwrap_or(last.as_str()), fqn);
}
