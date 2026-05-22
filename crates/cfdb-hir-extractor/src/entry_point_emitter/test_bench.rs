//! Test and bench attribute classification probes used by
//! `scan_file`'s `SyntaxKind::FN` dispatch (RFC-042). These probes
//! have no REGISTERS_PARAM counterpart — they are pure classification
//! (kept separate from `registers_param.rs` because they change for a
//! different reason: vocabulary evolution vs param-edge wiring
//! evolution).

use std::path::Path;

use ra_ap_syntax::ast::{self, AstNode, HasAttrs};

/// `true` when the fn carries an attribute whose last path segment is
/// `test`, `given`, `when`, or `then`. Matches `#[test]`,
/// `#[tokio::test]`, `#[async_std::test]`, `#[actix_rt::test]`, plus
/// cucumber-rs BDD step-def attributes `#[given]` / `#[when]` /
/// `#[then]`.
///
/// `#[cfg(test)]` correctly does NOT fire: the probe reads
/// `attr.meta().path()`, which for `#[cfg(test)]` yields path segment
/// `cfg` (not `test`). The `test` inside `cfg(...)` is a token-tree
/// argument to `cfg`, not a path segment, so the textual probe ignores
/// it.
pub(super) fn has_test_attr(fn_ast: &ast::Fn) -> bool {
    last_path_segment_matches(fn_ast, |seg| {
        matches!(seg, "test" | "given" | "when" | "then")
    })
}

/// `true` when the fn carries an attribute whose last path segment is
/// `bench`. Matches `#[bench]` (libtest / criterion-as-attribute).
pub(super) fn has_bench_attr(fn_ast: &ast::Fn) -> bool {
    last_path_segment_matches(fn_ast, |seg| seg == "bench")
}

/// Walk `fn_ast.attrs()`, extract the last `::`-separated segment of
/// each attribute's meta path, and return `true` when any segment
/// matches `predicate`. Mirrors `has_tool_attr`'s discipline (path-
/// segment matching on `attr.meta().path()`, NOT on token-tree
/// arguments).
fn last_path_segment_matches<F>(fn_ast: &ast::Fn, predicate: F) -> bool
where
    F: Fn(&str) -> bool,
{
    fn_ast.attrs().any(|attr| {
        let Some(path) = attr.meta().and_then(|m| m.path()) else {
            return false;
        };
        let segment = path
            .syntax()
            .to_string()
            .rsplit("::")
            .next()
            .unwrap_or_default()
            .trim()
            .to_string();
        predicate(&segment)
    })
}

/// `true` when `file_path` lives under a `tests/` directory that is a
/// sibling of its owning crate's `Cargo.toml` — the Cargo convention
/// that integration tests live under a crate root's `tests/`
/// subdirectory.
///
/// Walks parent directories of `file_path` until it finds a `Cargo.toml`
/// (the owning crate root). If a `tests` directory is encountered
/// BEFORE the crate root, returns `true`. Stopping at the first crate
/// root prevents false positives on workspace-relative paths where the
/// outer workspace happens to contain a `tests/` directory (e.g.
/// `cfdb-hir-extractor/tests/fixtures/<sub_crate>/src/lib.rs` —
/// `sub_crate` is its own crate root, so its `src/lib.rs` is NOT under
/// `tests/` in any meaningful Cargo sense).
pub(super) fn is_under_tests_dir(file_path: &Path) -> bool {
    is_under_dir_named_in_crate(file_path, "tests", |p| p.join("Cargo.toml").is_file())
}

/// `true` when `file_path` lives under a `benches/` directory that is
/// a sibling of its owning crate's `Cargo.toml`. Same semantics as
/// [`is_under_tests_dir`].
pub(super) fn is_under_benches_dir(file_path: &Path) -> bool {
    is_under_dir_named_in_crate(file_path, "benches", |p| p.join("Cargo.toml").is_file())
}

/// Pure helper: walks `file_path`'s ancestors and returns `true` iff a
/// directory named `dir_name` appears in the path BEFORE the first
/// crate root (a directory for which `is_crate_root` returns `true`).
///
/// Returning at the first crate-root match enforces the
/// `tests/`-is-a-Cargo-sibling invariant. Without it, fixture
/// sub-crates whose source lives under an outer `tests/fixtures/`
/// directory would be falsely classified.
fn is_under_dir_named_in_crate<F>(file_path: &Path, dir_name: &str, is_crate_root: F) -> bool
where
    F: Fn(&Path) -> bool,
{
    let mut saw_target = false;
    for ancestor in file_path.ancestors().skip(1) {
        if is_crate_root(ancestor) {
            return saw_target;
        }
        if ancestor.file_name().and_then(|n| n.to_str()) == Some(dir_name) {
            saw_target = true;
        }
    }
    // No crate root found — file is outside any cargo crate. Don't
    // classify it as a test/bench fn.
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_ap_syntax::{Edition, SourceFile};
    use std::path::PathBuf;

    /// Parse a Rust source fragment and return the first `ast::Fn`
    /// descendant. Used to construct synthetic `ast::Fn` inputs for
    /// the pure-probe assertions (rust-systems R1 D2 prescription).
    fn parse_fn(src: &str) -> ast::Fn {
        let parse = SourceFile::parse(src, Edition::Edition2021);
        let source_file = parse.tree();
        source_file
            .syntax()
            .descendants()
            .find_map(ast::Fn::cast)
            .expect("source fragment must contain at least one fn")
    }

    // --- has_test_attr positive cases ---

    #[test]
    fn has_test_attr_fires_on_bare_test() {
        let f = parse_fn("#[test] fn f() {}");
        assert!(has_test_attr(&f));
    }

    #[test]
    fn has_test_attr_fires_on_tokio_test() {
        let f = parse_fn("#[tokio::test] fn f() {}");
        assert!(has_test_attr(&f));
    }

    #[test]
    fn has_test_attr_fires_on_async_std_test() {
        let f = parse_fn("#[async_std::test] fn f() {}");
        assert!(has_test_attr(&f));
    }

    #[test]
    fn has_test_attr_fires_on_cucumber_given() {
        let f = parse_fn(r#"#[given("a step")] fn f() {}"#);
        assert!(has_test_attr(&f));
    }

    #[test]
    fn has_test_attr_fires_on_cucumber_when() {
        let f = parse_fn(r#"#[when("a step")] fn f() {}"#);
        assert!(has_test_attr(&f));
    }

    #[test]
    fn has_test_attr_fires_on_cucumber_then() {
        let f = parse_fn(r#"#[then("a step")] fn f() {}"#);
        assert!(has_test_attr(&f));
    }

    // --- has_bench_attr ---

    #[test]
    fn has_bench_attr_fires_on_bench() {
        let f = parse_fn("#[bench] fn f(_b: &mut Bencher) {}");
        assert!(has_bench_attr(&f));
        assert!(
            !has_test_attr(&f),
            "bench attr must NOT fire as test (mutual exclusion at probe level)"
        );
    }

    // --- precedence non-interference: #[tool] is not test/bench ---

    #[test]
    fn neither_fires_on_tool() {
        let f = parse_fn("#[tool] fn f() {}");
        assert!(!has_test_attr(&f), "tool attr must NOT fire as test");
        assert!(!has_bench_attr(&f), "tool attr must NOT fire as bench");
    }

    // --- cfg(test) negative case — the load-bearing split-brain boundary ---

    #[test]
    fn has_test_attr_does_not_fire_on_cfg_test() {
        // `#[cfg(test)]` yields path segment `cfg` (not `test`). The
        // `test` inside `cfg(...)` is a token-tree argument, not a
        // path segment — the textual probe must correctly ignore it.
        let f = parse_fn("#[cfg(test)] fn f() {}");
        assert!(
            !has_test_attr(&f),
            "cfg(test) must NOT fire as test (path segment is 'cfg', not 'test')"
        );
        assert!(!has_bench_attr(&f));
    }

    // --- bare fn — no attrs ---

    #[test]
    fn neither_fires_on_bare_fn() {
        let f = parse_fn("fn f() {}");
        assert!(!has_test_attr(&f));
        assert!(!has_bench_attr(&f));
    }

    // --- is_under_tests_dir / is_under_benches_dir ---

    // Closure that pretends `crates/foo` IS a Cargo crate root —
    // exercises the pure helper without filesystem I/O.
    fn fake_root(p: &Path) -> bool {
        p == Path::new("crates/foo")
    }

    #[test]
    fn is_under_dir_named_in_crate_fires_for_tests_layout() {
        assert!(is_under_dir_named_in_crate(
            &PathBuf::from("crates/foo/tests/integration.rs"),
            "tests",
            fake_root,
        ));
        assert!(is_under_dir_named_in_crate(
            &PathBuf::from("crates/foo/tests/common/mod.rs"),
            "tests",
            fake_root,
        ));
    }

    #[test]
    fn is_under_dir_named_in_crate_fires_for_benches_layout() {
        assert!(is_under_dir_named_in_crate(
            &PathBuf::from("crates/foo/benches/bench.rs"),
            "benches",
            fake_root,
        ));
    }

    #[test]
    fn is_under_dir_named_in_crate_rejects_src() {
        assert!(!is_under_dir_named_in_crate(
            &PathBuf::from("crates/foo/src/lib.rs"),
            "tests",
            fake_root,
        ));
        assert!(!is_under_dir_named_in_crate(
            &PathBuf::from("crates/foo/src/lib.rs"),
            "benches",
            fake_root,
        ));
    }

    #[test]
    fn is_under_dir_named_in_crate_does_not_cross_inner_crate_root() {
        // Mimics the fixture false-positive from #391 development:
        // `cfdb-hir-extractor/tests/fixtures/sub_crate/src/lib.rs` has
        // `tests` as an outer ancestor, but `sub_crate` is its own
        // crate root — the file is in `sub_crate`'s src/, not under
        // any tests/ relative to its OWN crate root.
        let path = PathBuf::from("cfdb-hir-extractor/tests/fixtures/sub_crate/src/lib.rs");
        let inner_root = |p: &Path| p == Path::new("cfdb-hir-extractor/tests/fixtures/sub_crate");
        assert!(!is_under_dir_named_in_crate(&path, "tests", inner_root));
    }

    #[test]
    fn is_under_dir_named_in_crate_rejects_when_no_crate_root() {
        // No `Cargo.toml` ancestor → not classified.
        assert!(!is_under_dir_named_in_crate(
            &PathBuf::from("orphan/tests/foo.rs"),
            "tests",
            |_| false,
        ));
    }
}
