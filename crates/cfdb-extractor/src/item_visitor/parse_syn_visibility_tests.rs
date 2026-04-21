use super::parse_syn_visibility;
use cfdb_core::Visibility;

fn parse(src: &str) -> syn::Visibility {
    // Parse via a wrapper item so the visibility appears in a
    // well-formed context syn accepts.
    let wrapped = format!("{src} fn dummy() {{}}");
    let item: syn::ItemFn = syn::parse_str(&wrapped).expect("parse test fixture");
    item.vis
}

#[test]
fn inherited_is_private() {
    assert_eq!(parse_syn_visibility(&parse("")), Visibility::Private);
}

#[test]
fn pub_is_public() {
    assert_eq!(parse_syn_visibility(&parse("pub")), Visibility::Public);
}

#[test]
fn pub_crate_is_crate_local() {
    assert_eq!(
        parse_syn_visibility(&parse("pub(crate)")),
        Visibility::CrateLocal
    );
}

#[test]
fn pub_super_and_pub_self_collapse_to_module() {
    assert_eq!(
        parse_syn_visibility(&parse("pub(super)")),
        Visibility::Module
    );
    assert_eq!(
        parse_syn_visibility(&parse("pub(self)")),
        Visibility::Module
    );
}

#[test]
fn pub_in_path_is_restricted() {
    assert_eq!(
        parse_syn_visibility(&parse("pub(in crate::foo::bar)")),
        Visibility::Restricted("crate::foo::bar".into())
    );
}

#[test]
fn pub_in_crate_does_not_collapse_to_crate_local() {
    // `pub(in crate)` is a restricted-path form, not the short
    // `pub(crate)`. We preserve the distinction on the wire.
    assert_eq!(
        parse_syn_visibility(&parse("pub(in crate)")),
        Visibility::Restricted("crate".into())
    );
}
