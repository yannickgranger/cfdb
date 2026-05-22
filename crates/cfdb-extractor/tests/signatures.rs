//! Integration-seam signature pin (RFC-044 §3.2 / #428, slice 044-B).
//!
//! Freezes the `cfdb_extractor::extract_workspace` public signature in
//! `tests/signatures.toml`. The test re-parses `src/lib.rs` with `syn`,
//! renders the target `fn`'s [`syn::Signature`] to a deterministic token
//! string via `quote::quote!`, and asserts byte-equality against the
//! frozen value. A silent signature change (param added, return type
//! shifted, fn renamed/removed) fails this test instead of slipping into
//! a release.
//!
//! **Mechanism (council Q1.c, unanimous):** per-crate frozen TOML + an
//! inline self-contained parser. NO shared `cfdb-signatures-check` crate
//! (no-monolith directive) — every pinned crate carries its own ≤~15-LOC
//! `toml` + `syn` + `quote` comparator.
//!
//! **Rotation cost:** an *intentional* signature change updates
//! `tests/signatures.toml` in the same commit. The diff on that file is
//! the single, explicit review surface.

use std::collections::BTreeMap;

use quote::ToTokens;

/// Source file holding the pinned public surface.
const SOURCE: &str = include_str!("../src/lib.rs");
/// Frozen rendered signatures, keyed by item name.
const FROZEN: &str = include_str!("signatures.toml");

/// Render every free `fn` in `SOURCE` to `(name, rendered signature)`.
/// `quote!` normalises whitespace, so the string is deterministic.
fn rendered_free_fns() -> BTreeMap<String, String> {
    let file = syn::parse_file(SOURCE).expect("parse src/lib.rs");
    file.items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Fn(f) => {
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
    let current = rendered_free_fns();

    let mut drift = String::new();
    for (name, want) in &frozen {
        match current.get(name) {
            None => drift.push_str(&format!(
                "  `{name}`: NOT FOUND in src/lib.rs (renamed/removed?)\n"
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

    // Non-vacuity: every pinned key must resolve to a real fn above.
    assert!(
        !frozen.is_empty(),
        "tests/signatures.toml pinned zero entries"
    );
}
