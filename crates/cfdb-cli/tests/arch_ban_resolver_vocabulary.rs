use std::fs;
use std::path::Path;

use assert_cmd::Command;
use tempfile::tempdir;

const RESOLVERS: &[(&str, bool)] = &[
    ("syn", false),
    ("hir", false),
    ("hir", true),
    ("tree-sitter-php", false),
    ("tree-sitter-php", true),
    ("tree-sitter-typescript", false),
];

const CONTRACT_VIOLATIONS: &[(&str, bool)] = &[("syn", true), ("tree-sitter-typescript", true)];

fn keyspace(pairs: &[(&str, bool)]) -> String {
    let nodes: Vec<String> = pairs
        .iter()
        .enumerate()
        .map(|(i, (resolver, resolved))| {
            format!(
                r#"{{"id":"callsite:probe::caller:callee:{i}","label":"CallSite","props":{{"caller_qname":"probe::caller","callee_path":"callee","callee_last_segment":"callee","kind":"call","file":"src/probe.rs","line":{},"is_test":false,"resolver":"{resolver}","callee_resolved":{resolved}}}}}"#,
                i + 1
            )
        })
        .collect();
    format!(
        r#"{{"schema_version":{{"major":0,"minor":8,"patch":0}},"nodes":[{}],"edges":[]}}"#,
        nodes.join(",")
    )
}

fn rows_for(db: &Path, rule: &str) -> (usize, i32) {
    let assertion = Command::cargo_bin("cfdb")
        .expect("cfdb binary")
        .args([
            "violations",
            "--db",
            db.to_str().expect("utf-8 db path"),
            "--keyspace",
            "probe",
            "--rule",
            rule,
        ])
        .assert();
    let out = assertion.get_output();
    let stdout = String::from_utf8(out.stdout.clone()).expect("utf-8 stdout");
    (
        stdout.matches("\"caller_qname\"").count(),
        out.status.code().expect("an exit code"),
    )
}

fn rule(name: &str) -> String {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("workspace root")
        .join("examples/queries")
        .join(name)
        .to_str()
        .expect("utf-8 rule path")
        .to_string()
}

fn db_with(pairs: &[(&str, bool)]) -> tempfile::TempDir {
    let dir = tempdir().expect("tempdir");
    fs::write(dir.path().join("probe.json"), keyspace(pairs)).expect("write keyspace");
    dir
}

#[test]
fn every_resolver_the_vocabulary_admits_passes_the_domain_rule() {
    let dir = db_with(RESOLVERS);
    let (rows, code) = rows_for(dir.path(), &rule("arch-ban-rfc-043-resolver-domain.cypher"));
    assert_eq!(
        (rows, code),
        (0, 0),
        "`cfdb-045-polyglot-relationship-edges#3` fixes the vocabulary at four values and the \
         descriptor documents all four; a rule admitting two refuses every call site a PHP or \
         TypeScript producer ever emitted"
    );
}

#[test]
fn a_resolver_outside_the_vocabulary_is_still_refused() {
    let dir = db_with(&[("tree-sitter-ruby", false)]);
    let (rows, code) = rows_for(dir.path(), &rule("arch-ban-rfc-043-resolver-domain.cypher"));
    assert_eq!(
        (rows, code),
        (1, 30),
        "the set is widened, not opened: a value no producer declares is the row this rule exists \
         for, and exit 30 is what blocks a merge"
    );
}

#[test]
fn the_two_resolvers_that_resolve_pass_the_discriminator_rule() {
    let dir = db_with(RESOLVERS);
    let (rows, code) = rows_for(
        dir.path(),
        &rule("arch-ban-rfc-043-discriminator-syn-resolved.cypher"),
    );
    assert_eq!(
        (rows, code),
        (0, 0),
        "`hir` resolves through HIR and `tree-sitter-php` resolves syntactically against the \
         in-workspace item set, which is what the descriptor says and what the resolved PHP call \
         sites are"
    );
}

#[test]
fn a_resolver_that_cannot_resolve_claiming_it_did_is_still_refused() {
    let dir = db_with(CONTRACT_VIOLATIONS);
    let (rows, code) = rows_for(
        dir.path(),
        &rule("arch-ban-rfc-043-discriminator-syn-resolved.cypher"),
    );
    assert_eq!(
        (rows, code),
        (2, 30),
        "`syn` and `tree-sitter-typescript` both hard-code `callee_resolved = false`, so a `true` \
         from either is the contract breach this rule names. Widening the rule must not switch it \
         off, and these two pairs are the assertion that it did not. They are written by hand \
         because no producer can emit them: that is what makes them a violation"
    );
}
