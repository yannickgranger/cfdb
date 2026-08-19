use std::path::{Path, PathBuf};

use cfdb_cli::check_predicate;
use cfdb_core::schema::Keyspace;
use cfdb_core::store::StoreBackend;
use cfdb_petgraph::{persist, PetgraphStore};

struct SeedCase {
    name: &'static str,
    params: fn() -> Vec<String>,
    min_rows: usize,
}

const SEED_CASES: &[SeedCase] = &[
    SeedCase {
        name: "path-regex",
        params: path_regex_params,
        min_rows: 40,
    },
    SeedCase {
        name: "context-homonym-crate-in-multiple-contexts",
        params: context_homonym_params,
        min_rows: 5,
    },
    SeedCase {
        name: "fn-returns-type-in-crate-set",
        params: fn_returns_type_params,
        min_rows: 0,
    },
];

fn path_regex_params() -> Vec<String> {
    vec!["pat:regex:.*\\.rs".to_string()]
}

fn context_homonym_params() -> Vec<String> {
    vec![
        "context_a:context:cfdb".to_string(),
        "context_b:context:cfdb".to_string(),
    ]
}

fn fn_returns_type_params() -> Vec<String> {
    vec![
        "type_pattern:regex:NoSuchType_xyz_ZZZ".to_string(),
        "fin_precision_crates:list:cfdb-core".to_string(),
    ]
}

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

fn list_seed_predicate_basenames(workspace_root: &Path) -> Vec<String> {
    let dir = workspace_root.join(".cfdb").join("predicates");
    let mut names: Vec<String> = std::fs::read_dir(&dir)
        .expect(".cfdb/predicates/ must exist")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("cypher"))
        .filter_map(|p| p.file_stem().and_then(|s| s.to_str()).map(str::to_string))
        .collect();
    names.sort();
    names
}

#[test]
fn seed_cases_cover_every_shipped_predicate() {
    let workspace_root = cfdb_workspace_root();
    let shipped = list_seed_predicate_basenames(&workspace_root);
    let cases: Vec<String> = SEED_CASES.iter().map(|c| c.name.to_string()).collect();

    let missing_cases: Vec<&String> = shipped.iter().filter(|n| !cases.contains(n)).collect();
    assert!(
        missing_cases.is_empty(),
        "new predicate(s) shipped in .cfdb/predicates/ without matching SeedCase: {missing_cases:?}. \
         Add a SeedCase entry with a deterministic param set + expected row bound."
    );

    let stale_cases: Vec<&String> = cases.iter().filter(|n| !shipped.contains(n)).collect();
    assert!(
        stale_cases.is_empty(),
        "SeedCase entry references missing predicate file(s): {stale_cases:?}. \
         Remove the case or restore the .cypher file."
    );
}

#[test]
fn every_seed_predicate_runs_against_cfdb_keyspace() {
    let workspace_root = cfdb_workspace_root();
    let tmp = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("tempdir");
    let db_dir = tmp.path().join("db");
    seed_keyspace(&workspace_root, &db_dir, "cfdb");

    for case in SEED_CASES {
        let params = (case.params)();
        let report = check_predicate(&db_dir, "cfdb", &workspace_root, case.name, &params)
            .unwrap_or_else(|e| panic!("predicate `{}` failed: {e}", case.name));
        assert_eq!(report.predicate_name, case.name);
        assert!(
            report.row_count >= case.min_rows,
            "predicate `{}` returned {} rows, expected >= {} (params={:?})",
            case.name,
            report.row_count,
            case.min_rows,
            params
        );

        let mut sorted = report.rows.clone();
        sorted.sort();
        assert_eq!(
            report.rows, sorted,
            "predicate `{}` rows must be sorted by (qname, line) ascending — §4.1",
            case.name
        );
    }
}

#[test]
fn every_seed_predicate_is_deterministic_across_two_calls() {
    let workspace_root = cfdb_workspace_root();
    let tmp = tempfile::tempdir_in(env!("CARGO_TARGET_TMPDIR")).expect("tempdir");
    let db_dir = tmp.path().join("db");
    seed_keyspace(&workspace_root, &db_dir, "cfdb");

    for case in SEED_CASES {
        let params = (case.params)();
        let first = check_predicate(&db_dir, "cfdb", &workspace_root, case.name, &params)
            .unwrap_or_else(|e| panic!("first run of `{}` failed: {e}", case.name));
        let second = check_predicate(&db_dir, "cfdb", &workspace_root, case.name, &params)
            .unwrap_or_else(|e| panic!("second run of `{}` failed: {e}", case.name));
        assert_eq!(
            first, second,
            "predicate `{}` is non-deterministic across two same-input calls — §4.1 violation",
            case.name
        );
    }
}
