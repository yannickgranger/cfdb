use std::path::Path;

use cfdb_core::enrich::EnrichBackend;
use cfdb_core::fact::{Node, PropValue, Props};
use cfdb_core::schema::{Keyspace, Label};
use cfdb_core::store::StoreBackend;

use crate::PetgraphStore;

// ------------------------------------------------------------------
// Fixture builders — a tempdir + a fresh git repo + one or more files
// committed along a linear history.
// ------------------------------------------------------------------

struct GitFixture {
    _tmp: tempfile::TempDir,
    workspace: std::path::PathBuf,
    repo: git2::Repository,
}

impl GitFixture {
    fn new() -> Self {
        let tmp = tempfile::tempdir().expect("tempdir");
        let workspace = tmp.path().to_path_buf();
        let repo = git2::Repository::init(&workspace).expect("git init");
        let mut cfg = repo.config().expect("repo.config");
        cfg.set_str("user.name", "Test Author").expect("cfg name");
        cfg.set_str("user.email", "test@example.com")
            .expect("cfg email");
        GitFixture {
            _tmp: tmp,
            workspace,
            repo,
        }
    }

    fn write(&self, rel: &str, contents: &str) {
        let path = self.workspace.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("mkdirs");
        }
        std::fs::write(&path, contents).expect("write file");
    }

    fn commit(&self, rel: &str, message: &str, time: i64) -> git2::Oid {
        let mut index = self.repo.index().expect("index");
        index.add_path(Path::new(rel)).expect("add_path");
        index.write().expect("index.write");
        let tree_oid = index.write_tree().expect("write_tree");
        let tree = self.repo.find_tree(tree_oid).expect("find_tree");
        let parents: Vec<git2::Commit<'_>> = match self.repo.head() {
            Ok(head) => vec![head.peel_to_commit().expect("peel_to_commit")],
            Err(_) => Vec::new(),
        };
        let parent_refs: Vec<&git2::Commit<'_>> = parents.iter().collect();
        let sig =
            git2::Signature::new("Test Author", "test@example.com", &git2::Time::new(time, 0))
                .expect("sig");
        self.repo
            .commit(Some("HEAD"), &sig, &sig, message, &tree, &parent_refs)
            .expect("commit")
    }
}

fn store_with_item(workspace: &Path, file_path: &str, item_qname: &str) -> PetgraphStore {
    let mut store = PetgraphStore::new().with_workspace(workspace);
    let ks = Keyspace::new("test");
    let mut props = Props::new();
    props.insert("qname".into(), PropValue::Str(item_qname.to_string()));
    props.insert("file".into(), PropValue::Str(file_path.to_string()));
    let node = Node {
        id: format!("item:{item_qname}"),
        label: Label::new(Label::ITEM),
        props,
    };
    store.ingest_nodes(&ks, vec![node]).expect("ingest_nodes");
    store
}

fn get_item_props(store: &PetgraphStore, keyspace: &Keyspace, qname: &str) -> Props {
    let (nodes, _edges) = store.export(keyspace).expect("export");
    nodes
        .into_iter()
        .find(|n| {
            n.props
                .get("qname")
                .and_then(PropValue::as_str)
                .is_some_and(|q| q == qname)
        })
        .unwrap_or_else(|| panic!("item {qname} not found"))
        .props
}

// ------------------------------------------------------------------
// Two-commit fixture — counts + last-ts + last-author correct.
// ------------------------------------------------------------------

#[test]
fn ac2_two_commit_fixture_writes_correct_attrs() {
    let fx = GitFixture::new();
    fx.write("src/lib.rs", "fn v1() {}\n");
    fx.commit("src/lib.rs", "first", 1_700_000_000);
    fx.write("src/lib.rs", "fn v2() {}\n");
    fx.commit("src/lib.rs", "second", 1_700_000_100);

    let mut store = store_with_item(&fx.workspace, "src/lib.rs", "crate::v2");
    let ks = Keyspace::new("test");
    let report = store.enrich_git_history(&ks).expect("pass");

    assert!(report.ran, "pass should run: {:?}", report.warnings);
    assert_eq!(report.attrs_written, 3, "one :Item × three attrs");

    let props = get_item_props(&store, &ks, "crate::v2");
    assert_eq!(
        props.get(super::ATTR_TS),
        Some(&PropValue::Int(1_700_000_100)),
        "most recent commit timestamp"
    );
    assert_eq!(
        props.get(super::ATTR_AUTHOR),
        Some(&PropValue::Str("test@example.com".into())),
        "committer email"
    );
    assert_eq!(
        props.get(super::ATTR_COUNT),
        Some(&PropValue::Int(2)),
        "two commits touched src/lib.rs"
    );
}

// ------------------------------------------------------------------
// Regression — `:Item.file` is an ABSOLUTE path emitted by
// `cfdb-extractor`, but `git_info` is keyed by the RELATIVE form
// `git diff` returns. Without path-strip in `write_attrs_one`,
// every item gets a Null timestamp on real cfdb-self extracts —
// surfaced when #349's dogfood reported 100% null on cfdb-self
// despite `attrs_written: 5637`. Pin the fix.
// ------------------------------------------------------------------

#[test]
fn absolute_file_path_strips_workspace_prefix_and_matches_git_info() {
    let fx = GitFixture::new();
    fx.write("src/lib.rs", "fn v1() {}\n");
    fx.commit("src/lib.rs", "first", 1_700_000_000);

    // Store the :Item with the ABSOLUTE file path the extractor
    // actually emits (extractor walks via cargo_metadata which
    // produces absolute paths). The pre-fix behavior would Null
    // every attr because git_info is keyed by `src/lib.rs`, not
    // `<workspace>/src/lib.rs`.
    let absolute_file = fx.workspace.join("src/lib.rs");
    let mut store = store_with_item(
        &fx.workspace,
        absolute_file.to_str().expect("utf8 path"),
        "crate::v1",
    );
    let ks = Keyspace::new("test");
    let report = store.enrich_git_history(&ks).expect("pass");

    assert!(report.ran);
    let props = get_item_props(&store, &ks, "crate::v1");
    assert_eq!(
        props.get(super::ATTR_TS),
        Some(&PropValue::Int(1_700_000_000)),
        "absolute :Item.file path must match git_info after \
         workspace_root strip; pre-fix this returned Null \
         (regression for #349 dogfood reporting 100% null)"
    );
}

// ------------------------------------------------------------------
// AC-3: untracked-file fixture — attrs all Null, no panic.
// ------------------------------------------------------------------

#[test]
fn ac3_untracked_file_gets_null_attrs() {
    let fx = GitFixture::new();
    fx.write("src/tracked.rs", "fn tracked() {}\n");
    fx.commit("src/tracked.rs", "initial", 1_700_000_000);
    fx.write("src/untracked.rs", "fn untracked() {}\n");

    let mut store = store_with_item(&fx.workspace, "src/untracked.rs", "crate::untracked");
    let ks = Keyspace::new("test");
    let report = store.enrich_git_history(&ks).expect("pass");

    assert!(report.ran);
    let props = get_item_props(&store, &ks, "crate::untracked");
    assert_eq!(props.get(super::ATTR_TS), Some(&PropValue::Null));
    assert_eq!(props.get(super::ATTR_AUTHOR), Some(&PropValue::Null));
    assert_eq!(props.get(super::ATTR_COUNT), Some(&PropValue::Null));
}

// ------------------------------------------------------------------
// AC-6: determinism — two runs produce identical canonical dumps.
// ------------------------------------------------------------------

#[test]
fn ac6_two_runs_produce_identical_canonical_dumps() {
    let fx = GitFixture::new();
    fx.write("src/a.rs", "a\n");
    fx.commit("src/a.rs", "a1", 1_700_000_000);
    fx.write("src/b.rs", "b\n");
    fx.commit("src/b.rs", "b1", 1_700_000_100);
    fx.write("src/a.rs", "a2\n");
    fx.commit("src/a.rs", "a2", 1_700_000_200);

    let mut store1 = store_with_item(&fx.workspace, "src/a.rs", "crate::a");
    let mut store2 = store_with_item(&fx.workspace, "src/a.rs", "crate::a");
    let ks = Keyspace::new("test");

    store1.enrich_git_history(&ks).expect("run 1");
    store2.enrich_git_history(&ks).expect("run 2");

    let dump1 = store1.canonical_dump(&ks).expect("dump 1");
    let dump2 = store2.canonical_dump(&ks).expect("dump 2");
    assert_eq!(dump1, dump2, "two runs must be byte-identical (G1)");
}

// ------------------------------------------------------------------
// Degraded paths: workspace not in a git repo.
// ------------------------------------------------------------------

#[test]
fn workspace_not_a_git_repo_writes_nulls_with_warning() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut store = store_with_item(tmp.path(), "src/lib.rs", "crate::x");
    let ks = Keyspace::new("test");
    let report = store.enrich_git_history(&ks).expect("pass");

    assert!(report.ran, "still ran — not an error, just degraded");
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("not inside a git repository")),
        "warning must name the repo issue: {:?}",
        report.warnings
    );
    let props = get_item_props(&store, &ks, "crate::x");
    assert_eq!(props.get(super::ATTR_TS), Some(&PropValue::Null));
}

#[test]
fn empty_keyspace_returns_ran_true_with_zero_counters() {
    let fx = GitFixture::new();
    fx.write("src/lib.rs", "fn _x() {}\n");
    fx.commit("src/lib.rs", "initial", 1_700_000_000);

    let mut store = PetgraphStore::new().with_workspace(&fx.workspace);
    let ks = Keyspace::new("test");
    store.ingest_nodes(&ks, Vec::new()).expect("ingest empty");
    let report = store.enrich_git_history(&ks).expect("pass");

    assert!(report.ran);
    assert_eq!(report.attrs_written, 0);
    assert_eq!(report.facts_scanned, 0);
}

#[test]
fn unknown_keyspace_returns_err() {
    let fx = GitFixture::new();
    let mut store = PetgraphStore::new().with_workspace(&fx.workspace);
    let ks = Keyspace::new("never_ingested");
    let err = store
        .enrich_git_history(&ks)
        .expect_err("unknown keyspace must error");
    let msg = format!("{err:?}");
    assert!(msg.contains("UnknownKeyspace"), "{msg}");
}

#[test]
fn no_workspace_root_returns_degraded_report() {
    let mut store = PetgraphStore::new();
    let ks = Keyspace::new("test");
    let mut props = Props::new();
    props.insert("qname".into(), PropValue::Str("crate::y".into()));
    props.insert("file".into(), PropValue::Str("src/lib.rs".into()));
    let node = Node {
        id: "item:crate::y".into(),
        label: Label::new(Label::ITEM),
        props,
    };
    store.ingest_nodes(&ks, vec![node]).expect("ingest");
    let report = store.enrich_git_history(&ks).expect("pass");

    assert!(!report.ran, "no workspace_root → ran=false");
    assert!(
        report.warnings.iter().any(|w| w.contains("workspace_root")),
        "warning must name the missing root: {:?}",
        report.warnings
    );
}
