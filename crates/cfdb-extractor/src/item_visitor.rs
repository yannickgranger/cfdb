//! `syn::Visit` implementation for module-level items. Drives `Item` /
//! `Module` / `Field` / `CallSite` emission and queues external `mod foo;`
//! declarations for the outer [`crate::file_walker`] to resolve and recurse
//! into.

use std::str::FromStr;

use cfdb_core::qname::{item_qname, module_qpath};
use cfdb_core::Visibility;

use crate::file_walker::PendingExternalMod;
use crate::Emitter;

mod emit;
mod visits;

#[cfg(test)]
mod parse_syn_visibility_tests;

pub(crate) use emit::emit_call_site_node_and_edge;

pub(crate) struct ItemVisitor<'e> {
    pub(crate) emitter: &'e mut Emitter,
    pub(crate) crate_id: String,
    pub(crate) crate_name: String,
    pub(crate) file_path: String,
    /// Bounded context the containing crate belongs to — computed once per
    /// crate in [`crate::extract_workspace`] via
    /// [`cfdb_concepts::compute_bounded_context`] and propagated down through
    /// [`crate::file_walker::visit_file`]. Stamped onto every Item node at
    /// emission time (council-cfdb-wiring §B.1.2).
    pub(crate) bounded_context: String,
    /// Path of module names from crate root to current position. The first
    /// element is the crate name (dashes replaced with underscores), matching
    /// Rust's qname convention.
    pub(crate) module_stack: Vec<String>,
    /// External (`mod foo;`) declarations encountered while visiting this
    /// file. Each carries its name, optional `#[path]` override, and
    /// whether it was under `#[cfg(test)]`. The caller resolves each to
    /// a child file and recurses, inheriting the test flag so every
    /// Item/CallSite beneath it is tagged correctly.
    pub(crate) pending_external_mods: Vec<PendingExternalMod>,
    /// Set while inside an `impl` block — the textual rendering of the impl
    /// target type. Used to build qnames for methods so `impl Foo { fn bar }`
    /// produces `module::Foo::bar` rather than `module::bar`.
    pub(crate) current_impl_target: Option<String>,
    /// Depth counter for nested `#[cfg(test)]` (or `#[cfg(all(test, ...))]`)
    /// module scopes. `> 0` means every Item/CallSite emitted right now is
    /// test code. This is the signal that lets `arch-ban-*` rules filter
    /// out test modules without resorting to qname regex hacks.
    pub(crate) test_mod_depth: u32,
}

/// Build the `qname` for an `impl` block (#42). The segments combine the
/// current module path, the normalised target type, and the canonical
/// `impl[_<Trait>]` suffix — yielding a stable, human-readable id that
/// disambiguates inherent impls from each distinct trait impl on the
/// same target:
///
/// - `impl Foo { ... }` at module `m`        → `m::Foo::impl`
/// - `impl Display for Foo { ... }`          → `m::Foo::impl_Display`
/// - `impl crate::bar::Trait for Foo { ... }` → `m::Foo::impl_crate_bar_Trait`
fn impl_block_qname(module_stack: &[String], target: &str, trait_qname: Option<&str>) -> String {
    let module = module_qpath(module_stack);
    let prefix = if module.is_empty() {
        String::new()
    } else {
        format!("{module}::")
    };
    let trait_segment = trait_qname
        .map(|t| format!("_{}", t.replace("::", "_")))
        .unwrap_or_default();
    format!("{prefix}{target}::impl{trait_segment}")
}

/// Human-readable `name` prop for an impl-block :Item node (#42). Mirrors
/// Rust source-level rendering: `impl Foo` (inherent) or
/// `impl Bar for Foo` (trait impl).
fn impl_block_name(target: &str, trait_qname: Option<&str>) -> String {
    match trait_qname {
        Some(t) => format!("impl {t} for {target}"),
        None => format!("impl {target}"),
    }
}

/// Resolve a bare type/trait name (as written in source) into the full
/// qname formula used by [`item_qname`]. For an unqualified segment like
/// `"Polite"`, the current crate + module prefix is prepended so the
/// resulting `item:<qname>` id matches what the struct/trait emitters
/// produce. Already-qualified inputs (containing `::`) pass through
/// unchanged — they may dangle when they point outside the workspace,
/// which the petgraph ingest layer handles with a non-fatal warning.
fn resolve_target_qname(module_stack: &[String], type_or_trait: &str) -> String {
    if type_or_trait.contains("::") {
        return type_or_trait.to_string();
    }
    item_qname(module_stack, type_or_trait)
}

fn span_line(_ident: &syn::Ident) -> usize {
    // proc_macro2::Span does not expose line info on stable Rust. Storing 0
    // is a known placeholder that callers can overwrite later with a
    // rustc-generated source map. RFC §8.2 phase B tracks this.
    0
}

/// Translate a `syn::Visibility` AST node into the typed cfdb-core enum
/// (RFC-033 §7 A1 / Issue #35).
///
/// Two-step pipeline: render the AST to the canonical wire string, then
/// delegate to `Visibility::from_str`. This keeps a **single resolution
/// point** for the Rust→`Visibility` mapping — when the variant list grows,
/// only `Visibility`'s wire-str / `FromStr` pair needs updating, and the
/// extractor automatically picks up the new variant. Split-brain audit
/// (`audit-split-brain` FromStrBypass check) enforces the invariant.
///
/// Mapping (see `render_syn_visibility_wire` for the AST side and
/// `impl FromStr for Visibility` for the string side):
///
/// - `pub`                        → `Public`
/// - `pub(crate)`                 → `CrateLocal`
/// - `pub(super)` / `pub(self)`   → `Module` (semantic equivalence; wire
///   always renders as `pub(super)`)
/// - inherited (no modifier)      → `Private`
/// - `pub(in path::to::mod)` and any other `Restricted` path → `Restricted`
///   carrying the `::`-joined path string
fn parse_syn_visibility(vis: &syn::Visibility) -> Visibility {
    let wire = render_syn_visibility_wire(vis);
    Visibility::from_str(&wire).expect(
        "render_syn_visibility_wire produces canonical wire strings that FromStr accepts — \
         if this panics, the two sides of the visibility mapping drifted and audit-split-brain \
         should have caught it",
    )
}

/// Render a `syn::Visibility` AST node to its canonical wire string
/// (see `Visibility::as_wire_str` for the inverse + full grammar). Kept
/// separate from `parse_syn_visibility` so tests can assert the rendering
/// alone without the FromStr round-trip.
fn render_syn_visibility_wire(vis: &syn::Visibility) -> String {
    match vis {
        syn::Visibility::Public(_) => "pub".to_string(),
        syn::Visibility::Inherited => "private".to_string(),
        syn::Visibility::Restricted(r) => {
            let segments: Vec<String> = r
                .path
                .segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect();
            // `pub(in crate)` / `pub(in super)` / `pub(in self)` — the
            // `in` keyword makes these canonically-path-restricted. syn
            // distinguishes them from the shorter `pub(crate)` /
            // `pub(super)` / `pub(self)` forms via `r.in_token.is_some()`.
            // The short form matches on a single-segment path without the
            // `in` keyword; the long form always keeps the path verbatim.
            let has_in = r.in_token.is_some();
            match (segments.len(), segments.first().map(String::as_str), has_in) {
                (1, Some("crate"), false) => "pub(crate)".to_string(),
                (1, Some("super"), false) | (1, Some("self"), false) => "pub(super)".to_string(),
                _ => format!("pub(in {})", segments.join("::")),
            }
        }
    }
}
