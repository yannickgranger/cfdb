use std::fs;
use std::path::Path;

use assert_cmd::assert::Assert;
use assert_cmd::Command;
use tempfile::tempdir;

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("mkdir -p");
    }
    fs::write(path, contents).expect("write fixture file");
}

fn undetectable_workspace(root: &Path) {
    write(&root.join("README.md"), "a tree no producer claims\n");
    write(&root.join("tool.py"), "def thing():\n    return 1\n");
}

fn every_marker_workspace(root: &Path) {
    write(
        &root.join("Cargo.toml"),
        "[workspace]\nresolver = \"2\"\nmembers = [\"only-crate\"]\n",
    );
    write(
        &root.join("only-crate/Cargo.toml"),
        "[package]\nname = \"only-crate\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    );
    write(&root.join("only-crate/src/lib.rs"), "pub fn thing() {}\n");

    write(
        &root.join("composer.json"),
        r#"{"name":"cfdb/probe","autoload":{"psr-4":{"App\\":"php/"}}}"#,
    );
    write(
        &root.join("php/Thing.php"),
        "<?php\n\nnamespace App;\n\nclass Thing\n{\n}\n",
    );

    write(
        &root.join("package.json"),
        r#"{"name":"probe","version":"1.0.0"}"#,
    );
    write(
        &root.join("tsconfig.json"),
        r#"{"compilerOptions":{"rootDir":"ts"}}"#,
    );
    write(&root.join("ts/thing.ts"), "export class Thing {}\n");
}

fn extract(workspace: &Path, db: &Path, keyspace: &str) -> Assert {
    Command::cargo_bin("cfdb")
        .expect("cfdb binary")
        .args([
            "extract",
            "--workspace",
            workspace.to_str().expect("utf-8 workspace path"),
            "--db",
            db.to_str().expect("utf-8 db path"),
            "--keyspace",
            keyspace,
        ])
        .assert()
}

fn stderr_of(assertion: &Assert) -> String {
    String::from_utf8(assertion.get_output().stderr.clone()).expect("utf-8 stderr")
}

#[test]
fn a_workspace_no_compiled_in_producer_detects_is_refused_and_writes_nothing() {
    let dir = tempdir().expect("tempdir");
    let workspace = dir.path().join("workspace");
    let db = dir.path().join("db");
    undetectable_workspace(&workspace);
    fs::create_dir_all(&db).expect("mkdir -p db");

    let assertion = extract(&workspace, &db, "vacuous").failure();
    let stderr = stderr_of(&assertion);

    assert!(
        stderr.contains("no LanguageProducer detected workspace"),
        "the refusal names the condition: {stderr}"
    );
    assert!(
        stderr.contains(workspace.to_str().expect("utf-8 workspace path")),
        "the refusal names the workspace it could not place: {stderr}"
    );
    assert!(
        stderr.contains("compiled-in producers"),
        "the refusal names what was compiled in, or the reader cannot tell a wrong build from a wrong tree: {stderr}"
    );
    assert!(
        !db.join("vacuous.json").exists(),
        "no keyspace is written: an empty one carries the current schema version and reads to every rule as a clean tree"
    );
}

#[test]
fn a_workspace_carrying_every_marker_is_not_refused_by_the_empty_arm() {
    let dir = tempdir().expect("tempdir");
    let workspace = dir.path().join("workspace");
    let db = dir.path().join("db");
    every_marker_workspace(&workspace);
    fs::create_dir_all(&db).expect("mkdir -p db");

    let assertion = extract(&workspace, &db, "detected");
    let stderr = stderr_of(&assertion);

    if assertion.get_output().status.success() {
        assert!(
            db.join("detected.json").exists(),
            "a run a producer detected writes its facts, so the refusal has not widened to every extraction"
        );
        return;
    }

    assert!(
        stderr.contains("compiled-in producers: []"),
        "this fixture carries the marker file of every producer, so the only build that may refuse it is one with no producer compiled in at all — and the refusal must say so: {stderr}"
    );
}

#[test]
fn the_polyglot_arm_still_warns_and_proceeds() {
    let dir = tempdir().expect("tempdir");
    let workspace = dir.path().join("workspace");
    let db = dir.path().join("db");
    every_marker_workspace(&workspace);
    fs::create_dir_all(&db).expect("mkdir -p db");

    let assertion = extract(&workspace, &db, "polyglot");
    let stderr = stderr_of(&assertion);

    if stderr.contains("polyglot workspace") {
        assert!(
            assertion.get_output().status.success(),
            "the polyglot arm proceeds; only the empty arm refuses: {stderr}"
        );
        assert!(
            stderr.contains("dispatch picks"),
            "the warning names the producer it picked, or a reader cannot tell which language the facts came from: {stderr}"
        );
        assert!(
            db.join("polyglot.json").exists(),
            "a run that produced facts writes them"
        );
        return;
    }

    if assertion.get_output().status.success() {
        assert!(
            db.join("polyglot.json").exists(),
            "no polyglot warning and no refusal means exactly one producer detected this fixture, and a single-producer run writes its facts: {stderr}"
        );
        return;
    }

    assert!(
        stderr.contains("compiled-in producers: []"),
        "this fixture carries every producer's marker file, so a build that refuses it compiled none in at all — and the refusal must say so: {stderr}"
    );
}
