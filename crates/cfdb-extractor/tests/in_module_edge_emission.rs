//! `IN_MODULE` edge emission tests (#267, audit ID CFDB-EXT-H1).
//!
//! `cfdb-core/src/schema/describe/edges.rs` declares `IN_MODULE` from
//! `[Item, File]` to `[Module]` as part of the v0.1 wire vocabulary,
//! and `cfdb-extractor/src/lib.rs` advertises it in the crate doc — but
//! the producer never emitted it. `SchemaDescribe()` lied to consumers
//! and any Cypher walking `Item -[:IN_MODULE]-> Module` returned zero
//! rows. This is the regression suite for the fix.
//!
//! The harness mirrors `field_emission.rs` / `param_emission.rs` /
//! `variant_emission.rs`: a real cargo workspace in a tempdir + the
//! full `extract_workspace` pipeline, so assertions reflect the
//! observable extractor output end-to-end (test hierarchy §2.5 tier 2:
//! integration against real inputs).

use std::path::Path;

use cfdb_core::fact::PropValue;
use cfdb_core::schema::{EdgeLabel, Label};
use cfdb_extractor::extract_workspace;
use tempfile::tempdir;

fn write_fixture_file(root: &Path, rel: &str, contents: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(
        p.parent()
            .expect("fixture path always has a parent directory"),
    )
    .expect("fixture mkdir -p");
    std::fs::write(p, contents).expect("fixture write");
}

fn write_cargo_workspace(root: &Path, crate_name: &str, lib_src: &str) {
    write_fixture_file(
        root,
        "Cargo.toml",
        &format!(
            r#"[workspace]
resolver = "2"
members = ["{crate_name}"]
"#
        ),
    );
    write_fixture_file(
        root,
        &format!("{crate_name}/Cargo.toml"),
        &format!(
            r#"[package]
name = "{crate_name}"
version = "0.0.1"
edition = "2021"
publish = false

[lib]
path = "src/lib.rs"
"#
        ),
    );
    write_fixture_file(root, &format!("{crate_name}/src/lib.rs"), lib_src);
}

fn prop_str<'a>(props: &'a std::collections::BTreeMap<String, PropValue>, key: &str) -> &'a str {
    match props.get(key).expect("prop present") {
        PropValue::Str(s) => s.as_str(),
        other => panic!("expected Str for key {key}, got {other:?}"),
    }
}

#[test]
fn nested_inline_module_item_emits_in_module_edge_to_deepest_module() {
    let fixture = tempdir().expect("tempdir");
    write_cargo_workspace(
        fixture.path(),
        "inmodfix",
        r#"pub mod a {
    pub mod b {
        pub fn foo() {}
    }
}
"#,
    );

    let (nodes, edges) = extract_workspace(fixture.path()).expect("extract");

    // Sanity: both `:Module` nodes exist (`module:inmodfix::a`,
    // `module:inmodfix::a::b`) so `IN_MODULE` has somewhere to land.
    let module_ids: std::collections::BTreeSet<&str> = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::MODULE)
        .map(|n| n.id.as_str())
        .collect();
    assert!(
        module_ids.contains("module:inmodfix::a"),
        "expected `:Module` node `module:inmodfix::a`, got {module_ids:?}"
    );
    assert!(
        module_ids.contains("module:inmodfix::a::b"),
        "expected `:Module` node `module:inmodfix::a::b`, got {module_ids:?}"
    );

    // The `foo` `:Item` node id is derived from its qname via the
    // canonical `item_node_id` formula — `item:<crate>::<modules>::<name>`.
    let foo_item_id = "item:inmodfix::a::b::foo";
    assert!(
        nodes
            .iter()
            .any(|n| n.label.as_str() == Label::ITEM && n.id == foo_item_id),
        "expected `:Item` node {foo_item_id} for `pub fn foo` inside `mod a::b`"
    );

    // The fix: an `IN_MODULE` edge from `foo` to its deepest enclosing
    // `:Module` node (`module:inmodfix::a::b`, NOT the crate root and
    // NOT the intermediate `module:inmodfix::a`).
    let in_module_edges: Vec<_> = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::IN_MODULE && e.src == foo_item_id)
        .collect();
    assert_eq!(
        in_module_edges.len(),
        1,
        "expected exactly 1 IN_MODULE edge from {foo_item_id}, got {} ({:?})",
        in_module_edges.len(),
        in_module_edges
    );
    assert_eq!(
        in_module_edges[0].dst, "module:inmodfix::a::b",
        "IN_MODULE dst must be the deepest enclosing module, not an outer one"
    );
}

#[test]
fn external_mod_file_emits_in_module_edge_to_its_own_module_node() {
    // Two-file layout: `lib.rs` declares `mod child;`, `child.rs` is
    // the external module file. The schema declares `IN_MODULE` from
    // `[Item, File]` so the `:File` node for `child.rs` MUST point
    // at `module:extfix::child` — the same `:Module` node the
    // `mod child;` declaration in `lib.rs` produced.
    let fixture = tempdir().expect("tempdir");
    write_fixture_file(
        fixture.path(),
        "Cargo.toml",
        r#"[workspace]
resolver = "2"
members = ["extfix"]
"#,
    );
    write_fixture_file(
        fixture.path(),
        "extfix/Cargo.toml",
        r#"[package]
name = "extfix"
version = "0.0.1"
edition = "2021"
publish = false

[lib]
path = "src/lib.rs"
"#,
    );
    write_fixture_file(
        fixture.path(),
        "extfix/src/lib.rs",
        r#"pub mod child;
"#,
    );
    write_fixture_file(
        fixture.path(),
        "extfix/src/child.rs",
        r#"pub fn ping() {}
"#,
    );

    let (nodes, edges) = extract_workspace(fixture.path()).expect("extract");

    // Find the `:File` node for `child.rs`. The id formula is
    // `file:<crate>:<rel_path>`; relative path is workspace-rooted in
    // `file_walker::visit_file_inner`.
    let child_file = nodes
        .iter()
        .find(|n| {
            n.label.as_str() == Label::FILE && prop_str(&n.props, "path").ends_with("child.rs")
        })
        .expect("child.rs `:File` node");

    // The edge: file → module:extfix::child.
    let file_in_module: Vec<_> = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::IN_MODULE && e.src == child_file.id)
        .collect();
    assert_eq!(
        file_in_module.len(),
        1,
        "expected exactly 1 IN_MODULE edge from `:File` {:?}, got {}",
        child_file.id,
        file_in_module.len()
    );
    assert_eq!(
        file_in_module[0].dst, "module:extfix::child",
        "child.rs `:File` IN_MODULE dst must be its own `:Module` node"
    );

    // And the function inside that file routes to the same module.
    let ping_id = "item:extfix::child::ping";
    let ping_in_module: Vec<_> = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::IN_MODULE && e.src == ping_id)
        .collect();
    assert_eq!(ping_in_module.len(), 1);
    assert_eq!(ping_in_module[0].dst, "module:extfix::child");
}

#[test]
fn crate_root_item_emits_no_in_module_edge() {
    // Items at crate root have no enclosing `:Module` node (cfdb's
    // existing convention emits `:Module` only for nested `mod` decls).
    // Emitting `IN_MODULE` to a non-existent dst would dangle and break
    // every reachability query; the helper is documented to be a no-op
    // at the root.
    let fixture = tempdir().expect("tempdir");
    write_cargo_workspace(
        fixture.path(),
        "rootfix",
        r#"pub fn at_root() {}
"#,
    );

    let (nodes, edges) = extract_workspace(fixture.path()).expect("extract");

    let root_id = "item:rootfix::at_root";
    assert!(
        nodes
            .iter()
            .any(|n| n.label.as_str() == Label::ITEM && n.id == root_id),
        "fixture sanity: `:Item` for crate-root fn must be present"
    );

    let in_module_from_root: Vec<_> = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::IN_MODULE && e.src == root_id)
        .collect();
    assert!(
        in_module_from_root.is_empty(),
        "crate-root items must not emit IN_MODULE (no `:Module` node at root); got {in_module_from_root:?}"
    );
}

#[test]
fn impl_block_and_method_inside_module_both_emit_in_module() {
    // The impl-block path goes through `emit_impl_block` and the
    // impl-method path bypasses `emit_item_with_flags` entirely (it
    // composes the qname inline so `module::Foo::bar` keeps the impl
    // target). Both wiring sites must emit `IN_MODULE` — this test
    // pins the contract for both.
    let fixture = tempdir().expect("tempdir");
    write_cargo_workspace(
        fixture.path(),
        "implfix",
        r#"pub mod inner {
    pub struct Foo;
    impl Foo {
        pub fn bar(&self) {}
    }
}
"#,
    );

    let (_nodes, edges) = extract_workspace(fixture.path()).expect("extract");

    let impl_block_id = "item:implfix::inner::Foo::impl";
    let method_id = "item:implfix::inner::Foo::bar";

    let impl_block_in_module: Vec<_> = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::IN_MODULE && e.src == impl_block_id)
        .collect();
    assert_eq!(
        impl_block_in_module.len(),
        1,
        "impl-block `:Item` must emit IN_MODULE; got {impl_block_in_module:?}"
    );
    assert_eq!(impl_block_in_module[0].dst, "module:implfix::inner");

    let method_in_module: Vec<_> = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::IN_MODULE && e.src == method_id)
        .collect();
    assert_eq!(
        method_in_module.len(),
        1,
        "impl-method `:Item` must emit IN_MODULE; got {method_in_module:?}"
    );
    assert_eq!(method_in_module[0].dst, "module:implfix::inner");
}
