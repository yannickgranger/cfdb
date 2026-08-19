use std::collections::BTreeMap;

use quote::ToTokens;

const SOURCE: &str = include_str!("../src/lib.rs");
const FROZEN: &str = include_str!("signatures.toml");

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
