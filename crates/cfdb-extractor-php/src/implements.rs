//! `IMPLEMENTS` pass-1 buffering for the PHP producer (RFC-045 §3.2 45-A).
//!
//! Buffers one `(class id, target interface qname)` pair per interface named
//! in a `class_interface_clause` (`implements`); the pairs are resolved to
//! `IMPLEMENTS` edges in pass 2 once every `:Item` exists
//! ([`crate::emitter::Emitter::resolve_pending_implements`]). `base_clause`
//! (`extends`) is intentionally not handled — inheritance is deferred (§3.3
//! D3-a).

use crate::emitter::Emitter;
use crate::{qualify, text};

/// Buffer one pending `IMPLEMENTS` pair per interface named in a
/// `class_interface_clause`. The clause's named children are `name`
/// (unqualified, e.g. `Greeter`) or `qualified_name` (e.g. `\Ns\I` or
/// `Sub\I`) nodes interleaved with `implements`/`,` tokens — only the two
/// type-reference kinds are resolved.
pub(crate) fn buffer_implements_targets(
    clause: tree_sitter::Node,
    src: &[u8],
    current_ns: Option<&str>,
    source_id: &str,
    emitter: &mut Emitter,
) {
    let mut cursor = clause.walk();
    for iface in clause.children(&mut cursor) {
        if matches!(iface.kind(), "name" | "qualified_name") {
            if let Some(target_qname) = resolve_interface_qname(iface, src, current_ns) {
                emitter.buffer_implements(source_id, &target_qname);
            }
        }
    }
}

/// Resolve an interface reference in an `implements` clause to the qname of
/// the `:Item` it would target. A fully-qualified (absolute) reference
/// `\Ns\I` strips the leading `\`; an unqualified or relative reference is
/// qualified against the current namespace exactly as class/interface qnames
/// are built (`qualify`). There is no `use`-import resolution in the MVP, so
/// an aliased import resolves to `current_ns\<text>` and simply finds no
/// matching `:Item` (closed-world — no edge).
fn resolve_interface_qname(
    node: tree_sitter::Node,
    src: &[u8],
    current_ns: Option<&str>,
) -> Option<String> {
    let raw = text(node, src)?;
    match raw.strip_prefix('\\') {
        Some(absolute) => Some(absolute.to_string()),
        None => Some(qualify(current_ns, raw)),
    }
}
