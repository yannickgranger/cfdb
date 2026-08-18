//! Integration-seam signature pin (RFC-044 §3.2 / #428, slice 044-B).
//!
//! Freezes the seven `pub(crate)` factory-function signatures in the
//! `cfdb-cli` composition root (`src/compose.rs`) — the single place that
//! knows which concrete `StoreBackend` is wired in — into
//! `tests/signatures.toml`. The test re-parses `src/compose.rs` with `syn`
//! (a textual `syn::parse_file`, so `pub(crate)` visibility is fully
//! visible — an integration test in `tests/` could not reach these via the
//! type system, but it CAN read and parse the source), renders each
//! `pub(crate) fn`'s [`syn::Signature`] to a deterministic token string via
//! `quote::quote!`, and asserts byte-equality against the frozen values.
//!
//! The private `discover_workspace_from_db` helper is intentionally NOT
//! pinned: it is a `fn` (not `pub(crate)`), an internal of the #409
//! closeout, not part of the composition-root factory contract. Filtering
//! on `pub(crate)` visibility excludes it.
//!
//! **Mechanism (council Q1.c, unanimous):** per-crate frozen TOML + an
//! inline self-contained parser. NO shared `cfdb-signatures-check` crate.
//!
//! **Rotation cost:** an *intentional* signature change updates
//! `tests/signatures.toml` in the same commit.

use std::collections::BTreeMap;

use quote::ToTokens;

/// Composition-root source file holding the pinned factory functions.
const SOURCE: &str = include_str!("../src/compose.rs");
/// Frozen rendered signatures, keyed by function name.
const FROZEN: &str = include_str!("signatures.toml");

/// `true` when `vis` is exactly `pub(crate)`.
fn is_pub_crate(vis: &syn::Visibility) -> bool {
    matches!(
        vis,
        syn::Visibility::Restricted(r) if r.in_token.is_none() && r.path.is_ident("crate")
    )
}

/// Render every `pub(crate)` free `fn` in `SOURCE` to `(name, rendered signature)`.
fn rendered_pub_crate_fns() -> BTreeMap<String, String> {
    let file = syn::parse_file(SOURCE).expect("parse src/compose.rs");
    file.items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Fn(f) if is_pub_crate(&f.vis) => {
                Some((f.sig.ident.to_string(), f.sig.to_token_stream().to_string()))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn pinned_signatures_match_source() {
    let frozen: BTreeMap<String, String> =
        toml::from_str(FROZEN).expect("parse tests/signatures.toml");
    let current = rendered_pub_crate_fns();

    let mut drift = String::new();
    for (name, want) in &frozen {
        match current.get(name) {
            None => drift.push_str(&format!(
                "  `{name}`: NOT FOUND as a pub(crate) fn in src/compose.rs (renamed/removed?)\n"
            )),
            Some(got) if got != want => drift.push_str(&format!(
                "  `{name}`:\n    frozen:  {want}\n    current: {got}\n"
            )),
            Some(_) => {}
        }
    }
    assert!(
        drift.is_empty(),
        "\nsignature drift for {} pinned item(s):\n{drift}\
         If a change is intentional, update tests/signatures.toml in the same commit.",
        frozen.len()
    );

    // Non-vacuity: the pinned count must equal the pub(crate) fn count in
    // compose.rs, so an ADDED factory (unpinned) fails rather than passing.
    assert_eq!(
        frozen.len(),
        current.len(),
        "pinned-entry count ({}) != pub(crate) fn count in src/compose.rs ({}). \
         A factory was added or removed — update tests/signatures.toml.",
        frozen.len(),
        current.len()
    );
}
