//! Integration-seam signature pin (RFC-044 §3.2 / #428, slice 044-B).
//!
//! Freezes the `PetgraphAdapter::ingest_resolved_call_sites` method
//! signature — the single orphan-rule bridge through which HIR-resolved
//! call sites enter a `PetgraphStore` — in `tests/signatures.toml`. The
//! test re-parses `src/lib.rs` with `syn`, walks every `impl` block for a
//! method of that name, renders its [`syn::Signature`] to a deterministic
//! token string via `quote::quote!`, and asserts byte-equality against
//! the frozen value. A silent change to the seam (a new param, a return
//! type shift) fails here.
//!
//! **Mechanism (council Q1.c, unanimous):** per-crate frozen TOML + an
//! inline self-contained parser. NO shared `cfdb-signatures-check` crate.
//!
//! **Rotation cost:** an *intentional* signature change updates
//! `tests/signatures.toml` in the same commit.

use std::collections::BTreeMap;

use quote::ToTokens;

/// Source file holding the pinned impl method.
const SOURCE: &str = include_str!("../src/lib.rs");
/// Frozen rendered signatures, keyed by method name.
const FROZEN: &str = include_str!("signatures.toml");

/// Render every `impl`-block method in `SOURCE` to `(name, rendered signature)`.
fn rendered_impl_methods() -> BTreeMap<String, String> {
    let file = syn::parse_file(SOURCE).expect("parse src/lib.rs");
    let mut out = BTreeMap::new();
    for item in &file.items {
        if let syn::Item::Impl(imp) = item {
            for impl_item in &imp.items {
                if let syn::ImplItem::Fn(f) = impl_item {
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
    let current = rendered_impl_methods();

    let mut drift = String::new();
    for (name, want) in &frozen {
        match current.get(name) {
            None => drift.push_str(&format!(
                "  `{name}`: NOT FOUND as an impl method in src/lib.rs (renamed/removed?)\n"
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

    assert!(
        !frozen.is_empty(),
        "tests/signatures.toml pinned zero entries"
    );
}
