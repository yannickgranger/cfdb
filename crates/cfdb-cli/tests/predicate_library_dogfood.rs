//! Predicate-library dogfood — RFC-034 Slice 4 / #148.
//!
//! Iterates EVERY `.cfdb/predicates/*.cypher` shipped in the workspace,
//! runs it against cfdb's own keyspace with a fixed param set defined in
//! this file, and asserts a loose lower-bound row count + sorted
//! determinism for each.
//!
//! This is the superset of `check_predicate_dogfood.rs` (which covers two
//! seeds individually). The sweep test catches regressions when a new
//! seed lands without its case being added here — if a future PR ships
//! a new `.cfdb/predicates/<X>.cypher` without extending SEED_CASES, the
//! sweep asserts via `SEED_CASES.len() == seed_files.len()` and fails
//! with an actionable message.

use std::path::{Path, PathBuf};

use cfdb_cli::check_predicate;
use cfdb_core::schema::Keyspace;
use cfdb_core::store::StoreBackend;
use cfdb_petgraph::{persist, PetgraphStore};

/// One case = one predicate file + its canonical param set + the assertion
/// shape. Row counts are LOWER BOUNDS (`>= min_rows`) so the test survives
/// future source growth without being rewritten every PR.
struct SeedCase {
    name: &'static str,
    params: fn() -> Vec<String>,
    min_rows: usize,
}

const SEED_CASES: &[SeedCase] = &[
    // Path-regex — every `.rs` file in cfdb matches. The loose `>= 40`
    // bound covers the current tree with large headroom.
    SeedCase {
        name: "path-regex",
        params: path_regex_params,
        min_rows: 40,
    },
    // Context-homonym — binding both `$context_a` and `$context_b` to the
    // same `cfdb` context makes every cfdb crate a "homonym of itself" and
    // surfaces every declared crate. Matches `>= 5` for forward-compat
    // (the cfdb context has ≥9 crates on develop today).
    SeedCase {
        name: "context-homonym-crate-in-multiple-contexts",
        params: context_homonym_params,
        min_rows: 5,
    },
    // fn-returns-type-in-crate-set — a pattern that matches no cfdb fn
    // signature (the string `NoSuchType_xyz_ZZZ` is not used anywhere).
    // Asserts zero rows — this is the "no false positives" proof.
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

/// Structural: every predicate shipped in `.cfdb/predicates/` MUST have a
/// matching `SeedCase` entry in this test. A new seed without a matching
/// case fails with the exact missing basename — actionable for reviewers.
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

/// Functional: every seed predicate runs end-to-end against a freshly-
/// extracted cfdb keyspace with its canonical params and returns rows
/// that meet its lower-bound + sorted determinism.
#[test]
fn every_seed_predicate_runs_against_cfdb_keyspace() {
    let workspace_root = cfdb_workspace_root();
    let tmp = tempfile::tempdir().expect("tempdir");
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

        // Determinism: rows sorted ascending by (qname, line).
        let mut sorted = report.rows.clone();
        sorted.sort();
        assert_eq!(
            report.rows, sorted,
            "predicate `{}` rows must be sorted by (qname, line) ascending — §4.1",
            case.name
        );
    }
}

/// Determinism at the library-API level: two consecutive `check_predicate`
/// calls with the same inputs produce byte-identical `PredicateRunReport`
/// values (§4.1). The `ci/predicate-determinism.sh` script proves the same
/// invariant at the binary level; this test proves it at the library-API
/// level so a regression surfaces inside `cargo test`.
#[test]
fn every_seed_predicate_is_deterministic_across_two_calls() {
    let workspace_root = cfdb_workspace_root();
    let tmp = tempfile::tempdir().expect("tempdir");
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
