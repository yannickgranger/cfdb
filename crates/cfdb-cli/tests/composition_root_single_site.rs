//! Composition-root regression guard — RFC-044 §3.3 sub-band 2 (slice 044-C / #422).
//!
//! **Invariant asserted:** within `crates/cfdb-cli/src/`, the store is
//! constructed in exactly one place — `compose.rs`. Every other cfdb-cli
//! module (`scope.rs`, `stubs.rs`, `enrich.rs`, `hir.rs`, …) receives an
//! already-built `PetgraphStore` from its caller; none calls
//! `PetgraphStore::new()` directly. `compose.rs` IS the cfdb-cli binary's
//! composition root.
//!
//! Per clean-arch R1 (RFC-044 §3.3): this is a **regression guard on an
//! already-true property**, not a fix. In particular `hir.rs::extract_and_ingest_hir`
//! takes `store: &mut PetgraphStore` — it is NOT a second composition root.
//! The guard locks that property so a future edit can't silently reintroduce
//! a second construction site.
//!
//! **Scope = production `src/` only.** Integration tests under
//! `crates/cfdb-cli/tests/` legitimately build their own stores as fixtures;
//! the composition-root invariant is about the binary's wiring, not test
//! setup. The `#[cfg(test)]` module inside `compose.rs` (the `:234` site) is
//! in `compose.rs` and so does not violate the single-site rule.
//!
//! **Static file scan, not a compile-time check** (same rationale as
//! `cfdb-hir-extractor/tests/arch_boundary.rs`): the violation shape is
//! "someone typed `PetgraphStore::new(` into a cfdb-cli/src file other than
//! compose.rs" — a textual scan catches it in every profile in milliseconds.

use std::fs;
use std::path::{Path, PathBuf};

/// Construction call token. The trailing `(` distinguishes a real call from a
/// bare type-path mention.
const CONSTRUCTOR: &str = "PetgraphStore::new(";

/// The single permitted construction site, relative to `crates/cfdb-cli/src/`.
const COMPOSITION_ROOT: &str = "compose.rs";

/// `crates/cfdb-cli/src/` — `CARGO_MANIFEST_DIR` is `.../crates/cfdb-cli`.
fn cfdb_cli_src() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Walk a directory recursively, collecting every `.rs` file.
fn collect_rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries =
        fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir({}) failed: {e}", dir.display()));
    for entry in entries {
        let entry = entry.expect("directory entry readable");
        let path = entry.path();
        if path.is_dir() {
            collect_rs_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn petgraph_store_constructed_only_in_compose() {
    let src = cfdb_cli_src();
    assert!(
        src.is_dir(),
        "expected `crates/cfdb-cli/src/` at {}",
        src.display()
    );

    let mut files = Vec::new();
    collect_rs_files(&src, &mut files);
    assert!(
        !files.is_empty(),
        "non-vacuity guard: scanned zero .rs files under {} — check the walk root",
        src.display()
    );

    // Relative-to-src paths of every file that constructs a PetgraphStore.
    let mut sites: Vec<String> = Vec::new();
    for file in &files {
        let contents = fs::read_to_string(file)
            .unwrap_or_else(|e| panic!("read {} failed: {e}", file.display()));
        if contents.contains(CONSTRUCTOR) {
            let rel = file
                .strip_prefix(&src)
                .unwrap_or(file.as_path())
                .to_string_lossy()
                .replace('\\', "/");
            sites.push(rel);
        }
    }
    sites.sort();

    // Non-vacuity: compose.rs MUST be a construction site, otherwise the
    // token or walk root is wrong and the test would pass on an empty scan.
    assert!(
        sites.iter().any(|s| s == COMPOSITION_ROOT),
        "non-vacuity guard: `{CONSTRUCTOR}` not found in `{COMPOSITION_ROOT}` — \
         the constructor token or scan root is stale (sites found: {sites:?})"
    );

    // The single-site invariant: compose.rs is the ONLY construction site.
    let extra: Vec<&String> = sites.iter().filter(|s| *s != COMPOSITION_ROOT).collect();
    assert!(
        extra.is_empty(),
        "composition-root violation (RFC-044 §3.3 / CLEAN-3): `{CONSTRUCTOR}` \
         appears in cfdb-cli/src outside `{COMPOSITION_ROOT}`: {extra:?}\n\
         Only `compose.rs` may construct the store; other modules must receive \
         it from their caller (e.g. `hir.rs` takes `store: &mut PetgraphStore`)."
    );
}
