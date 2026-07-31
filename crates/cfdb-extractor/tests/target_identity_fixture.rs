//! RFC-054 54-B (#557) — the ONE shared bin-target fixture.
//!
//! Council-prescribed test design: the SAME fixture asserts both that ids
//! and edge endpoints are target-discriminated AND that display props
//! (`qname`, `caller_qname`, `parent_qname`) stay bare — so a
//! conflated-variable bug (one string doing both jobs) cannot pass the two
//! assertion families separately.
//!
//! Fixture shape = the #542 evidence shape: TWO `src/bin/*.rs` targets with
//! byte-identical contents (same-qname items, same-spelling call sites),
//! plus a lib target the bins call into and a bin-local type the resolver
//! must route back to the SAME target.

use std::collections::BTreeSet;
use std::path::Path;

use cfdb_core::fact::{Node, PropValue};
use cfdb_core::schema::{EdgeLabel, Label};
use cfdb_extractor::extract_workspace;
use tempfile::tempdir;

fn write_fixture_file(root: &Path, rel: &str, contents: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().expect("fixture path has a parent")).expect("fixture mkdir");
    std::fs::write(p, contents).expect("fixture write");
}

/// Two identical bins + one lib. Each bin: a bin-local struct returned by a
/// bin fn (RETURNS row), a bin-local enum matched on (MATCHES_ON row), a
/// param carrying a lib type (TYPE_OF row), and a call into the lib
/// (`:CallSite` row).
fn write_target_identity_fixture(root: &Path) {
    write_fixture_file(
        root,
        "Cargo.toml",
        "[package]\nname = \"tif\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
    );
    write_fixture_file(
        root,
        "src/lib.rs",
        "pub struct LibType;\n\npub fn shared() -> u32 {\n    7\n}\n",
    );
    let bin_body = r#"struct BinLocal;

enum Pick {
    One,
    Two,
}

fn make(seed: u32) -> BinLocal {
    let _ = seed;
    BinLocal
}

fn pick(p: Pick) -> u32 {
    match p {
        Pick::One => 1,
        Pick::Two => 2,
    }
}

fn use_lib(t: tif::LibType) -> u32 {
    let _ = t;
    tif::shared()
}

fn main() {
    let _ = make(1);
    let _ = pick(Pick::One);
    let _ = use_lib(tif::LibType);
}
"#;
    write_fixture_file(root, "src/bin/alpha.rs", bin_body);
    write_fixture_file(root, "src/bin/beta.rs", bin_body);
}

fn prop_str<'a>(n: &'a Node, key: &str) -> Option<&'a str> {
    n.props.get(key).and_then(PropValue::as_str)
}

#[test]
fn target_identity_shared_fixture() {
    let fixture = tempdir().expect("tempdir");
    let root = fixture.path();
    write_target_identity_fixture(root);

    let (nodes, edges) = extract_workspace(root).expect("extract fixture workspace");
    let node_ids: BTreeSet<&str> = nodes.iter().map(|n| n.id.as_str()).collect();

    // --- identity: every node id is unique (pre-054 the twin bins collide) ---
    assert_eq!(
        node_ids.len(),
        nodes.len(),
        "duplicate node ids — bin-target items are colliding: {:?}",
        {
            let mut seen = BTreeSet::new();
            nodes
                .iter()
                .filter(|n| !seen.insert(n.id.as_str()))
                .map(|n| n.id.as_str())
                .collect::<Vec<_>>()
        }
    );

    let items: Vec<&Node> = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::ITEM)
        .collect();

    // --- two distinct mains, discriminated ids, bare display qnames ---
    let mains: Vec<&&Node> = items
        .iter()
        .filter(|n| prop_str(n, "name") == Some("main"))
        .collect();
    assert_eq!(mains.len(), 2, "one :Item main per bin target");
    let main_targets: BTreeSet<&str> = mains.iter().filter_map(|n| prop_str(n, "target")).collect();
    assert_eq!(
        main_targets,
        BTreeSet::from(["bin:alpha", "bin:beta"]),
        ":Item.target carries the wire discriminator"
    );
    for m in &mains {
        assert!(
            m.id.contains("#bin:"),
            "bin-target id must carry the suffix, got `{}`",
            m.id
        );
        assert_eq!(
            prop_str(m, "qname"),
            Some("tif::main"),
            "display qname stays bare"
        );
    }

    // --- lib items: byte-stable ids, target=lib ---
    let shared = items
        .iter()
        .find(|n| prop_str(n, "qname") == Some("tif::shared"))
        .expect("lib fn present");
    assert_eq!(shared.id, "item:tif::shared", "lib ids are byte-stable");
    assert_eq!(prop_str(shared, "target"), Some("lib"));

    // --- call sites: same-spelling calls in sibling bins stay distinct ---
    let call_sites: Vec<&Node> = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::CALL_SITE)
        .filter(|n| prop_str(n, "callee_path") == Some("tif::shared"))
        .collect();
    assert_eq!(
        call_sites.len(),
        2,
        "one `tif::shared` :CallSite per bin (pre-054 they collapse to one)"
    );
    let cs_ids: BTreeSet<&str> = call_sites.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(
        cs_ids.len(),
        2,
        ":CallSite ids must be distinct across bins"
    );
    for cs in &call_sites {
        assert!(
            cs.id.contains("#bin:"),
            "cs id embeds caller identity: {}",
            cs.id
        );
        assert_eq!(
            prop_str(cs, "caller_qname"),
            Some("tif::use_lib"),
            "caller_qname prop stays the bare display qname"
        );
    }

    // --- params: derived ids inherit the parent identity; props stay bare ---
    let make_params: Vec<&Node> = nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::PARAM)
        .filter(|n| prop_str(n, "parent_qname") == Some("tif::make"))
        .collect();
    assert_eq!(make_params.len(), 2, "one seed param per bin's make()");
    for p in &make_params {
        assert!(
            p.id.contains("#bin:"),
            "param id inherits the discriminated parent identity: {}",
            p.id
        );
    }

    // --- RETURNS into a bin-local type resolves to the SAME target ---
    let returns: Vec<_> = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::RETURNS)
        .filter(|e| e.src.contains("tif::make"))
        .collect();
    assert_eq!(returns.len(), 2, "make() -> BinLocal resolves in both bins");
    for e in &returns {
        assert!(
            node_ids.contains(e.dst.as_str()),
            "RETURNS dst must exist (no silent dangle): {}",
            e.dst
        );
        assert!(
            e.dst.contains("#bin:"),
            "bin-local dst is discriminated: {}",
            e.dst
        );
        let src_suffix = e.src.rsplit("#bin:").next();
        let dst_suffix = e.dst.rsplit("#bin:").next();
        assert_eq!(
            src_suffix, dst_suffix,
            "same-target resolution policy: src/dst share the bin suffix"
        );
    }

    // --- TYPE_OF into a LIB type falls back to the lib target ---
    let type_of_lib: Vec<_> = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::TYPE_OF)
        .filter(|e| e.dst == "item:tif::LibType")
        .collect();
    assert_eq!(
        type_of_lib.len(),
        2,
        "use_lib(t: tif::LibType) resolves to the undiscriminated lib id from both bins"
    );

    // --- MATCHES_ON into a bin-local enum resolves same-target ---
    let matches_on: Vec<_> = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::MATCHES_ON)
        .collect();
    assert_eq!(
        matches_on.len(),
        2,
        "one match site per bin resolves to Pick"
    );
    for e in &matches_on {
        assert!(
            e.dst.contains("tif::Pick") && e.dst.contains("#bin:"),
            "MATCHES_ON dst is the same-target enum: {}",
            e.dst
        );
        assert!(
            node_ids.contains(e.dst.as_str()),
            "MATCHES_ON dst must exist: {}",
            e.dst
        );
    }

    // --- no spurious synthesize stubs for bin-local types ---
    let bin_locals: Vec<&&Node> = items
        .iter()
        .filter(|n| prop_str(n, "qname") == Some("tif::BinLocal"))
        .collect();
    assert_eq!(
        bin_locals.len(),
        2,
        "exactly one BinLocal :Item per bin — no third synthesized stub"
    );

    // --- every edge endpoint that is an item: id resolves (global no-dangle) ---
    for e in &edges {
        for endpoint in [&e.src, &e.dst] {
            if endpoint.starts_with("item:") {
                assert!(
                    node_ids.contains(endpoint.as_str()),
                    "dangling {} endpoint `{}` (src `{}` -[{}]-> dst `{}`)",
                    if endpoint == &e.src { "src" } else { "dst" },
                    endpoint,
                    e.src,
                    e.label,
                    e.dst
                );
            }
        }
    }
}
