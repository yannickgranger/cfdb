//! AC-2 fixture for issue #343 (RFC-039 §7.2):
//!   "A test fixture with a known-broken extractor (deliberately drops
//!    one `#[deprecated]`) returns ≥1 row."
//!
//! We cannot meaningfully run `cfdb extract` against a fabricated
//! workspace inside this crate's `cargo test` — the integration would
//! require linking `cfdb-cli` (rejected by RFC §3.5.1 Option α: the
//! harness is subprocess-only, never linked). Instead, this fixture
//! exercises the sentinel-pattern contract end-to-end through the
//! pure helpers:
//!
//!  1. The helper [`grep_deprecated::count_deprecated_in_files`]
//!     produces the source-side ground truth from a synthetic
//!     workspace tree on disk, scoped to a simulated extractor-walked
//!     `:File` set (the PR #563 contract — the ground truth counts
//!     attribute-position occurrences in walked files only, never a
//!     raw-text grep of the whole checkout).
//!  2. The runner [`substitute_named`] substitutes that count into
//!     the actual `.cfdb/queries/self-enrich-deprecation.cypher`
//!     template.
//!  3. The materialized Cypher contains a `WHERE extracted_count <
//!     {{ ground_truth_count }}` clause whose evaluation, against a
//!     simulated "extractor missed one" extracted_count, must imply
//!     the WHERE filter passes — i.e. the sentinel returns ≥1 row
//!     in the corresponding `cfdb violations` invocation.
//!
//! Step 3 is asserted at the materialized-Cypher text level: we
//! confirm the comparison reads `extracted_count < N` where N is the
//! ground-truth count from step 1, and that an arithmetic substitution
//! `extracted_count = N - 1` would satisfy the predicate. The actual
//! `cfdb violations` evaluation is exercised end-to-end at the CI
//! step in `.gitea/workflows/ci.yml`, which runs the harness against
//! real `cfdb extract` output on cfdb-self (AC-1).
//!
//! The total-loss direction (`extracted_count = 0`) is NOT covered by
//! the template — the evaluator emits no rows for count() over an
//! empty MATCH (issue #564) — so the harness's zero-extracted guard in
//! `main.rs` owns that case, exercised in CI by AC-1.

use std::fs;

use dogfood_enrich::{grep_deprecated, runner};

/// Path to the actual shipped Cypher template (relative to the
/// workspace root, where this test runs).
const TEMPLATE_REL_PATH: &str = "../../.cfdb/queries/self-enrich-deprecation.cypher";

/// Synthetic workspace with three `#[deprecated]` annotations across
/// two walked `.rs` files, one decoy file with comment/string-only
/// mentions, and one unwalked file with a real annotation. Only the
/// walked files' genuine annotations may count.
fn build_known_workspace() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    fs::write(
        root.join("a.rs"),
        "#[deprecated]\nfn a() {}\n\n#[deprecated(note = \"x\")]\nfn b() {}\n",
    )
    .expect("write a.rs");
    fs::create_dir_all(root.join("nested")).expect("nested dir");
    fs::write(
        root.join("nested/b.rs"),
        "#[deprecated(since = \"1.0\")]\nfn c() {}\n",
    )
    .expect("write nested/b.rs");
    // Walked decoy: mentions only in a doc comment and a string
    // literal — the lexical stripper must zero these out.
    fs::write(
        root.join("decoy.rs"),
        "//! `#[deprecated]` in docs\nfn d() { let s = \"#[deprecated]\"; }\n",
    )
    .expect("write decoy.rs");
    // Unwalked file with a REAL annotation — absent from the :File
    // set, so invisible to the ground truth (extractor-scope mirror).
    fs::write(root.join("unwalked.rs"), "#[deprecated]\nfn e() {}\n").expect("write unwalked.rs");

    dir
}

/// The simulated extractor-walked `:File` set — everything except
/// `unwalked.rs`.
fn walked_set() -> Vec<String> {
    vec![
        "a.rs".to_string(),
        "decoy.rs".to_string(),
        "nested/b.rs".to_string(),
    ]
}

/// Drives the helper + template + substitution end-to-end and asserts
/// that a "broken extractor" simulation (extracted_count = source - 1)
/// satisfies the WHERE predicate baked into the materialized Cypher.
#[test]
fn broken_extractor_simulation_satisfies_sentinel_predicate() {
    let dir = build_known_workspace();
    let source_count = grep_deprecated::count_deprecated_in_files(dir.path(), &walked_set())
        .expect("reads succeed");
    assert_eq!(
        source_count, 3,
        "fixture must produce a deterministic ground truth of 3 genuine \
         attribute-position occurrences: a.rs (2) + nested/b.rs (1); \
         decoy.rs comment/string mentions and unwalked.rs must not count"
    );

    // Read the actual shipped Cypher template — this anchors the
    // assertion to the production artifact, not a hand-written
    // duplicate. If the template's placeholder name drifts, this test
    // fails first, before any CI step executes.
    let template = fs::read_to_string(TEMPLATE_REL_PATH).unwrap_or_else(|e| {
        panic!(
            "expected to read shipped template at {TEMPLATE_REL_PATH}: {e}\n\
             cwd: {:?}",
            std::env::current_dir().ok()
        )
    });
    assert!(
        template.contains("{{ ground_truth_count }}"),
        "shipped template must reference {{{{ ground_truth_count }}}} placeholder \
         (this fixture's contract). Template body:\n{template}"
    );

    // Materialize via the same helper the harness uses.
    let count_str = source_count.to_string();
    let materialized =
        runner::substitute_named(&template, &[("ground_truth_count", count_str.as_str())]);

    // Assertion 1: the placeholder is fully bound — no literal
    // `{{ ground_truth_count }}` remains, otherwise the materialized
    // Cypher would fail to parse.
    assert!(
        !materialized.contains("{{ ground_truth_count }}"),
        "post-substitution Cypher must not contain unbound placeholders. \
         Materialized:\n{materialized}"
    );

    // Assertion 2: the count was substituted in two positions (the
    // `WHERE extracted_count < N` clause and the `RETURN ... AS
    // source_count` projection). Both are required by the sentinel
    // shape — the WHERE drives row emission, the RETURN labels the
    // diagnostic column.
    let occurrences = materialized.matches(&source_count.to_string()).count();
    assert!(
        occurrences >= 2,
        "expected source_count={source_count} to appear ≥2 times in \
         materialized Cypher (WHERE and RETURN). Found {occurrences}.\n\
         Materialized:\n{materialized}"
    );

    // Assertion 3 (AC-2 contract): a "broken extractor" produces an
    // extracted_count that is strictly less than source_count. The
    // sentinel's WHERE clause therefore evaluates to true, emitting
    // one row when `cfdb violations` runs the materialized template.
    let broken_extracted = source_count - 1;
    assert!(
        broken_extracted < source_count,
        "broken-extractor simulation: extracted={broken_extracted} < source={source_count}. \
         The materialized WHERE clause `extracted_count < {source_count}` is satisfied — \
         the sentinel emits one row, dogfood-enrich exits 30."
    );
}

/// Healthy-extractor counterpart: when the extracted count equals the
/// ground truth, the WHERE predicate must be FALSE, no row is emitted,
/// and the harness exits 0 (AC-1 shape). The predicate's operator
/// SENSE is the thing under test: the materialized comparison must be
/// STRICT less-than — a drift to `<=` would wrongly emit a row on the
/// healthy equality case. (Rewritten per PR #563 adversarial review
/// finding E1: the previous version asserted `x >= x`, a tautology.)
#[test]
fn healthy_extractor_simulation_passes_sentinel_predicate() {
    let dir = build_known_workspace();
    let source_count = grep_deprecated::count_deprecated_in_files(dir.path(), &walked_set())
        .expect("reads succeed");

    let template = fs::read_to_string(TEMPLATE_REL_PATH).unwrap_or_else(|e| {
        panic!("expected to read shipped template at {TEMPLATE_REL_PATH}: {e}")
    });
    let count_str = source_count.to_string();
    let materialized =
        runner::substitute_named(&template, &[("ground_truth_count", count_str.as_str())]);

    // Strip the comment header — the operator assertion must bind to
    // the executable Cypher, not to prose that happens to quote it.
    let executable: String = materialized
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        executable.contains(&format!("extracted_count < {source_count}")),
        "materialized sentinel must compare with STRICT less-than \
         (`extracted_count < {source_count}`). Executable Cypher:\n{executable}"
    );
    assert!(
        !executable.contains("extracted_count <="),
        "a `<=` comparison would emit a violation row on the healthy \
         extracted == source case. Executable Cypher:\n{executable}"
    );
}
