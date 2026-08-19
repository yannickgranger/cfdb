use std::collections::BTreeMap;

use quote::ToTokens;

const SOURCES: &[&str] = &[
    include_str!("../src/hir_db.rs"),
    include_str!("../src/call_site_emitter.rs"),
    include_str!("../src/entry_point_emitter.rs"),
];
const FROZEN: &str = include_str!("signatures.toml");

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
