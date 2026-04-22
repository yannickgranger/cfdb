//! Self-dogfood test for RFC-035 slice 1 (#180).
//!
//! Loads the cfdb workspace's own `.cfdb/indexes.toml` through
//! [`IndexSpec::from_path`] and asserts the three entries from
//! RFC-035 §3.2 are present, every entry carries a non-empty `notes`
//! rationale, and a serde round-trip preserves the spec exactly.
//!
//! This is the "self dogfood (cfdb on cfdb)" row of the Tests template
//! (cfdb CLAUDE.md §2.5). At this slice boundary the loader has no
//! downstream consumer — the build pass (slice 2), evaluator fast
//! paths (slices 5/6), and composition-root wiring (slice 7) all land
//! later. What this test asserts now is that *the real config cfdb
//! authors for itself* parses without error through the surface
//! future slices will consume.

use cfdb_petgraph::index::{ComputedKey, IndexEntry, IndexSpec};
use std::path::PathBuf;

/// Return the repo's `.cfdb/indexes.toml` path. Walks up from this
/// file's directory until `Cargo.lock` is found (the workspace root).
fn repo_indexes_toml() -> PathBuf {
    let mut dir: PathBuf = env!("CARGO_MANIFEST_DIR").into();
    loop {
        if dir.join("Cargo.lock").exists() && dir.join(".cfdb").exists() {
            return dir.join(".cfdb").join("indexes.toml");
        }
        if !dir.pop() {
            panic!("could not locate workspace root from CARGO_MANIFEST_DIR");
        }
    }
}

#[test]
fn loads_cfdb_own_indexes_toml() {
    let path = repo_indexes_toml();
    assert!(
        path.exists(),
        "workspace is missing .cfdb/indexes.toml at {path:?} — slice 1 (#180) requires it"
    );

    let spec = IndexSpec::from_path(&path).unwrap_or_else(|e| {
        panic!("cfdb's own .cfdb/indexes.toml failed to parse: {e}");
    });

    assert_eq!(
        spec.entries.len(),
        3,
        "RFC-035 §3.2 prescribes three entries (qname, bounded_context, last_segment(qname))"
    );

    let kinds: Vec<&IndexEntry> = spec.entries.iter().collect();
    let prop_keys: Vec<&str> = kinds
        .iter()
        .filter_map(|e| match e {
            IndexEntry::Prop { prop, .. } => Some(prop.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        prop_keys.contains(&"qname"),
        "expected an Item.qname index entry (RFC-035 §3.2)"
    );
    assert!(
        prop_keys.contains(&"bounded_context"),
        "expected an Item.bounded_context index entry (RFC-035 §3.2 / #169)"
    );

    let computed_keys: Vec<ComputedKey> = kinds
        .iter()
        .filter_map(|e| match e {
            IndexEntry::Computed { computed, .. } => Some(*computed),
            _ => None,
        })
        .collect();
    assert_eq!(
        computed_keys,
        vec![ComputedKey::LastSegment],
        "expected exactly one computed-key entry: last_segment(qname)"
    );

    for (idx, entry) in spec.entries.iter().enumerate() {
        let (label, notes) = match entry {
            IndexEntry::Prop { label, notes, .. } | IndexEntry::Computed { label, notes, .. } => {
                (label, notes)
            }
        };
        assert_eq!(
            label, "Item",
            "v0.1 indexes are all on :Item (entry {idx})"
        );
        assert!(
            !notes.trim().is_empty(),
            "entry {idx} has empty `notes` — every entry MUST document its rationale"
        );
    }
}

#[test]
fn own_indexes_toml_round_trips_through_serde() {
    let path = repo_indexes_toml();
    let spec = IndexSpec::from_path(&path).expect("load");

    let json = serde_json::to_string(&spec).expect("serialize");
    let reparsed: IndexSpec = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(
        spec, reparsed,
        "round-trip through serde must preserve the spec exactly — including every `notes` string"
    );
}
