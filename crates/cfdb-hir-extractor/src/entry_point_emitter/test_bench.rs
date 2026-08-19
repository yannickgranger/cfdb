use std::path::Path;

use ra_ap_syntax::ast::{self, AstNode, HasAttrs};

pub(super) fn has_test_attr(fn_ast: &ast::Fn) -> bool {
    last_path_segment_matches(fn_ast, |seg| {
        matches!(seg, "test" | "given" | "when" | "then")
    })
}

pub(super) fn has_bench_attr(fn_ast: &ast::Fn) -> bool {
    last_path_segment_matches(fn_ast, |seg| seg == "bench")
}

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

pub(super) fn is_under_tests_dir(file_path: &Path) -> bool {
    is_under_dir_named_in_crate(file_path, "tests", |p| p.join("Cargo.toml").is_file())
}

pub(super) fn is_under_benches_dir(file_path: &Path) -> bool {
    is_under_dir_named_in_crate(file_path, "benches", |p| p.join("Cargo.toml").is_file())
}

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
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use ra_ap_syntax::{Edition, SourceFile};
    use std::path::PathBuf;

    fn parse_fn(src: &str) -> ast::Fn {
        let parse = SourceFile::parse(src, Edition::Edition2021);
        let source_file = parse.tree();
        source_file
            .syntax()
            .descendants()
            .find_map(ast::Fn::cast)
            .expect("source fragment must contain at least one fn")
    }

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

    #[test]
    fn has_bench_attr_fires_on_bench() {
        let f = parse_fn("#[bench] fn f(_b: &mut Bencher) {}");
        assert!(has_bench_attr(&f));
        assert!(
            !has_test_attr(&f),
            "bench attr must NOT fire as test (mutual exclusion at probe level)"
        );
    }

    #[test]
    fn neither_fires_on_tool() {
        let f = parse_fn("#[tool] fn f() {}");
        assert!(!has_test_attr(&f), "tool attr must NOT fire as test");
        assert!(!has_bench_attr(&f), "tool attr must NOT fire as bench");
    }

    #[test]
    fn has_test_attr_does_not_fire_on_cfg_test() {
        let f = parse_fn("#[cfg(test)] fn f() {}");
        assert!(
            !has_test_attr(&f),
            "cfg(test) must NOT fire as test (path segment is 'cfg', not 'test')"
        );
        assert!(!has_bench_attr(&f));
    }

    #[test]
    fn neither_fires_on_bare_fn() {
        let f = parse_fn("fn f() {}");
        assert!(!has_test_attr(&f));
        assert!(!has_bench_attr(&f));
    }

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
        let path = PathBuf::from("cfdb-hir-extractor/tests/fixtures/sub_crate/src/lib.rs");
        let inner_root = |p: &Path| p == Path::new("cfdb-hir-extractor/tests/fixtures/sub_crate");
        assert!(!is_under_dir_named_in_crate(&path, "tests", inner_root));
    }

    #[test]
    fn is_under_dir_named_in_crate_rejects_when_no_crate_root() {
        assert!(!is_under_dir_named_in_crate(
            &PathBuf::from("orphan/tests/foo.rs"),
            "tests",
            |_| false,
        ));
    }
}
