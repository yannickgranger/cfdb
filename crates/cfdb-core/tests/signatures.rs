//! Integration-seam signature pin (RFC-044 §3.2 / #428, slice 044-B).
//!
//! Freezes every trait-method signature in `src/store.rs` — `StoreBackend`'s
//! six storage / lifecycle methods every backend must implement, plus
//! `QueryBackend::execute`, the query-execution contract `cfdb-eval`
//! implements — in `tests/signatures.toml`. The test re-parses
//! `src/store.rs` with `syn`, walks the trait's method items, renders each
//! [`syn::Signature`] to a deterministic token string via `quote::quote!`,
//! and asserts byte-equality against the frozen values. A silent change to
//! any method (a new param, an error-type shift, an added method) fails
//! here — exactly the drift that would otherwise break every downstream
//! `impl StoreBackend` adapter only at their next recompile.
//!
//! **Mechanism (council Q1.c, unanimous):** per-crate frozen TOML + an
//! inline self-contained parser. NO shared `cfdb-signatures-check` crate.
//! Note cfdb-core's own `[dependencies]` stay free of `syn`/`quote`/`toml`
//! (they are `[dev-dependencies]`); the CLEAN-3 dep-rule scan reads only
//! `[dependencies]`, so the hub-foundation purity invariant is unaffected.
//!
//! **Rotation cost:** an *intentional* signature change updates
//! `tests/signatures.toml` in the same commit.

use std::collections::BTreeMap;

use quote::ToTokens;

/// Source file defining the `StoreBackend` trait.
const SOURCE: &str = include_str!("../src/store.rs");
/// Frozen rendered signatures, keyed by method name.
const FROZEN: &str = include_str!("signatures.toml");

/// Render every trait method in `SOURCE` to `(name, rendered signature)`.
fn rendered_trait_methods() -> BTreeMap<String, String> {
    let file = syn::parse_file(SOURCE).expect("parse src/store.rs");
    let mut out = BTreeMap::new();
    for item in &file.items {
        if let syn::Item::Trait(tr) = item {
            for trait_item in &tr.items {
                if let syn::TraitItem::Fn(f) = trait_item {
                    out.insert(f.sig.ident.to_string(), f.sig.to_token_stream().to_string());
                }
            }
        }
    }
    out
}

#[test]
fn pinned_signatures_match_source() {
    let frozen: BTreeMap<String, String> =
        toml::from_str(FROZEN).expect("parse tests/signatures.toml");
    let current = rendered_trait_methods();

    let mut drift = String::new();
    for (name, want) in &frozen {
        match current.get(name) {
            None => drift.push_str(&format!(
                "  `{name}`: NOT FOUND as a StoreBackend method in src/store.rs (renamed/removed?)\n"
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

    // Non-vacuity: the pinned count must equal the trait-method count in
    // source, so an ADDED method (unpinned) fails rather than passing silently.
    assert_eq!(
        frozen.len(),
        current.len(),
        "pinned-entry count ({}) != StoreBackend trait-method count in source ({}). \
         A method was added or removed — update tests/signatures.toml.",
        frozen.len(),
        current.len()
    );
}
