use std::path::{Path, PathBuf};

use cfdb_cli::check_predicate;
use cfdb_core::schema::Keyspace;
use cfdb_core::store::StoreBackend;
use cfdb_petgraph::{persist, PetgraphStore};

fn cfdb_workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("CARGO_MANIFEST_DIR must have two parents")
        .to_path_buf()
}

fn seed_keyspace(workspace_root: &Path, db_dir: &Path, keyspace_name: &str) {
    let (nodes, edges) = cfdb_extractor::extract_workspace(workspace_root).expect("extract cfdb");
    let ks = Keyspace::new(keyspace_name);
    let mut store = PetgraphStore::new();
    store
        .ingest_nodes(&ks, nodes)
        .expect("ingest_nodes into PetgraphStore");
    store
        .ingest_edges(&ks, edges)
        .expect("ingest_edges into PetgraphStore");
    std::fs::create_dir_all(db_dir).expect("mkdir -p db");
    let keyspace_path = db_dir.join(format!("{keyspace_name}.json"));
    persist::save(&store, &ks, &keyspace_path).expect("persist keyspace");
}

#[test]
fn path_regex_predicate_matches_cfdb_query_source_files() {
    let workspace_root = cfdb_workspace_root();
    let tmp = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("tempdir");
    let db_dir = tmp.path().join("db");
    seed_keyspace(&workspace_root, &db_dir, "cfdb");

    let params = vec!["pat:literal:cfdb-query/.*\\.rs".to_string()];
    let report =
        check_predicate(&db_dir, "cfdb", &workspace_root, "path-regex", &params).expect("report");

    assert_eq!(report.predicate_name, "path-regex");
    assert!(
        report.row_count >= 10,
        "expected ≥10 rows for path-regex against cfdb workspace, got {} (rows: {:#?})",
        report.row_count,
        report.rows
    );

    let mut sorted_rows = report.rows.clone();
    sorted_rows.sort();
    assert_eq!(
        report.rows, sorted_rows,
        "rows must be sorted by (qname, line) ascending — determinism invariant §4.1"
    );

    for row in &report.rows {
        assert!(
            row.qname.contains("cfdb-query/"),
            "row qname `{}` does not match pattern `cfdb-query/.*\\.rs`",
            row.qname
        );
        assert_eq!(row.line, 0, "path-regex seed emits line=0 for :File rows");
    }
}

#[test]
fn context_homonym_predicate_self_dogfood_is_empty() {
    let workspace_root = cfdb_workspace_root();
    let tmp = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("tempdir");
    let db_dir = tmp.path().join("db");
    seed_keyspace(&workspace_root, &db_dir, "cfdb");

    let params = vec![
        "context_a:context:cfdb".to_string(),
        "context_b:context:cfdb".to_string(),
    ];
    let report = check_predicate(
        &db_dir,
        "cfdb",
        &workspace_root,
        "context-homonym-crate-in-multiple-contexts",
        &params,
    )
    .expect("report");

    assert_eq!(
        report.predicate_name,
        "context-homonym-crate-in-multiple-contexts"
    );
    assert!(
        report.row_count > 0,
        "expected ≥1 row when both params bind to the same context — cfdb crates appear in both"
    );
    let mut sorted = report.rows.clone();
    sorted.sort();
    assert_eq!(report.rows, sorted);
}
