use std::collections::BTreeMap;

use quote::ToTokens;

const SOURCE: &str = include_str!("../src/compose.rs");
const FROZEN: &str = include_str!("signatures.toml");

fn is_pub_crate(vis: &syn::Visibility) -> bool {
    matches!(
        vis,
        syn::Visibility::Restricted(r) if r.in_token.is_none() && r.path.is_ident("crate")
    )
}

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

    assert_eq!(
        frozen.len(),
        current.len(),
        "pinned-entry count ({}) != pub(crate) fn count in src/compose.rs ({}). \
         A factory was added or removed — update tests/signatures.toml.",
        frozen.len(),
        current.len()
    );
}
