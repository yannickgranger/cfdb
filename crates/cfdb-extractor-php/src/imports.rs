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

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.aliases.len()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Declaration {
    pub fqn: String,
    pub alias: Option<String>,
    pub line: i64,
}

#[derive(Debug, Default)]
pub(crate) struct Imports {
    pub table: ImportTable,
    pub declarations: Vec<Declaration>,
}

pub(crate) fn collect(program: tree_sitter::Node, src: &[u8]) -> Imports {
    let mut imports = Imports::default();
    let mut cursor = program.walk();
    for child in program.children(&mut cursor) {
        if child.kind() == "namespace_use_declaration" {
            absorb_declaration(child, src, &mut imports);
        }
    }
    imports
}

fn imports_a_symbol_not_a_class(clause: tree_sitter::Node, src: &[u8]) -> bool {
    let mut cursor = clause.walk();
    let children: Vec<tree_sitter::Node> = clause.children(&mut cursor).collect();
    children
        .iter()
        .any(|child| !child.is_named() && matches!(text(*child, src), Some("function" | "const")))
}

fn absorb_declaration(decl: tree_sitter::Node, src: &[u8], imports: &mut Imports) {
    let mut cursor = decl.walk();
    let children: Vec<tree_sitter::Node> = decl.children(&mut cursor).collect();
    let group = children.iter().find(|c| c.kind() == "namespace_use_group");
    let Some(group) = group else {
        for clause in children
            .iter()
            .filter(|c| c.kind() == "namespace_use_clause")
        {
            absorb_clause(*clause, src, None, imports);
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
        absorb_clause(clause, src, prefix, imports);
    }
}

fn absorb_clause(
    clause: tree_sitter::Node,
    src: &[u8],
    prefix: Option<&str>,
    imports: &mut Imports,
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
    imports.declarations.push(Declaration {
        fqn: fqn.clone(),
        alias: alias.map(str::to_string),
        line: (clause.start_position().row + 1) as i64,
    });
    imports.table.insert(alias.unwrap_or(last.as_str()), fqn);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> (tree_sitter::Tree, Vec<u8>) {
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_php::LANGUAGE_PHP.into())
            .expect("tree-sitter-php grammar");
        let tree = parser.parse(source, None).expect("parse");
        (tree, source.as_bytes().to_vec())
    }

    fn imports_of(source: &str) -> Imports {
        let (tree, src) = parse(source);
        collect(tree.root_node(), &src)
    }

    #[test]
    fn the_declaration_list_is_lossless_where_the_alias_table_is_not() {
        let imports = imports_of("<?php\nuse App\\Domain\\Clock;\nuse App\\Adapter\\Clock;\n");

        assert_eq!(
            imports.declarations.len(),
            2,
            "two clauses import two different names: {:?}",
            imports.declarations
        );
        assert_eq!(
            imports.table.len(),
            1,
            "the alias table keys on the written name, so the second `Clock` overwrites the \
             first; this is why `:Import` is not read off that table"
        );
    }

    #[test]
    fn an_alias_is_recorded_as_written_and_resolved_case_folded() {
        let imports = imports_of("<?php\nuse App\\Domain\\Clock as SystemClock;\n");

        assert_eq!(
            imports.declarations,
            vec![Declaration {
                fqn: "App\\Domain\\Clock".to_string(),
                alias: Some("SystemClock".to_string()),
                line: 2,
            }]
        );
        assert_eq!(
            imports.table.resolve("systemclock", None),
            "App\\Domain\\Clock",
            "resolution stays case-insensitive as PHP is"
        );
    }
}
