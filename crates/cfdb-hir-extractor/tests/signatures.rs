//! Integration-seam signature pin (RFC-044 §3.2 / #428, slice 044-B).
//!
//! Freezes the public seam signatures of `cfdb-hir-extractor` —
//! `build_hir_database`, `extract_call_sites`, `extract_entry_points` —
//! in `tests/signatures.toml`. The test re-parses the defining source
//! files with `syn`, renders each target `fn`'s [`syn::Signature`] to a
//! deterministic token string via `quote::quote!`, and asserts
//! byte-equality against the frozen values. A silent change (a HIR
//! 2-tuple → 3-tuple return shift, a `where` bound, a renamed fn) fails
//! here instead of slipping into a release.
//!
//! **Mechanism (council Q1.c, unanimous):** per-crate frozen TOML + an
//! inline self-contained parser. NO shared `cfdb-signatures-check` crate.
//!
//! **Rotation cost:** an *intentional* signature change updates
//! `tests/signatures.toml` in the same commit.

use std::collections::BTreeMap;

use quote::ToTokens;

/// Defining source files for the three pinned seam functions.
const SOURCES: &[&str] = &[
    include_str!("../src/hir_db.rs"),              // build_hir_database
    include_str!("../src/call_site_emitter.rs"),   // extract_call_sites
    include_str!("../src/entry_point_emitter.rs"), // extract_entry_points
];
/// Frozen rendered signatures, keyed by item name.
const FROZEN: &str = include_str!("signatures.toml");

/// Render every free `fn` across `SOURCES` to `(name, rendered signature)`.
fn rendered_free_fns() -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for src in SOURCES {
        let file = syn::parse_file(src).expect("parse cfdb-hir-extractor source");
        for item in &file.items {
            if let syn::Item::Fn(f) = item {
                out.insert(f.sig.ident.to_string(), f.sig.to_token_stream().to_string());
            }
        }
    }
    out
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
                "  `{name}`: NOT FOUND in source (renamed/removed?)\n"
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
