//! Const-driven recall thresholds for the nightly Gitea status workflow
//! (issue #340, Phase C of EPIC #338).
//!
//! Per `CLAUDE.md` §6 row 5 ("No metric ratchets") and the project-level
//! `CLAUDE.md` §3 row 5: every threshold in the cfdb tooling lives as a
//! `const` in tool source. There is no `.recall-baseline.json`, no
//! allowlist file, no `--update-baseline` flag. Raising a threshold is a
//! reviewed PR that edits this file.
//!
//! Two scopes are exposed:
//!
//! * [`RECALL_THRESHOLD_PER_CRATE`] — minimum acceptable recall for any
//!   single crate. Drops below this fail the per-crate `recall/<crate>`
//!   Gitea status.
//! * [`RECALL_THRESHOLD_TOTAL`] — minimum acceptable aggregate recall
//!   across all measured crates (matched-sum / adjusted-denominator-sum).
//!   Drops below this fail the `recall/total` aggregate status.
//!
//! Per-crate dispatch via [`threshold_for_crate`] lets a crate carry a
//! locally tighter or looser floor while keeping the source of truth in
//! one match-arm. The default arm returns
//! [`RECALL_THRESHOLD_PER_CRATE`] — overrides are explicit and reviewed.
//!
//! These values are independent of [`crate::DEFAULT_THRESHOLD`] (the
//! v0.1 hard-gate floor used by the PR-time slim build, currently 0.95).
//! The nightly workflow can run at a tighter floor than the PR gate
//! because nightly is soft-warning during the first cycle (#340 AC-5).

/// Minimum recall ratio (matched / adjusted_denominator) any single
/// crate must clear in the nightly run before its `recall/<crate-name>`
/// Gitea status flips from `success` to `failure`.
///
/// Initial value: `0.85`. Conservative on purpose — the nightly is
/// soft-warning for the first cycle (AC-5), so a too-tight floor would
/// front-load alert fatigue before we have a baseline of nightly runs
/// to calibrate against. Expected to be tightened in a reviewed PR
/// after the second-cycle promotion to required-check.
pub const RECALL_THRESHOLD_PER_CRATE: f64 = 0.85;

/// Minimum aggregate recall the workspace must clear before the
/// `recall/total` Gitea status flips from `success` to `failure`.
///
/// Initial value: `0.90`. Higher than the per-crate floor because the
/// aggregate denominator is dominated by larger crates, so a single
/// small crate's regression is diluted. Setting the total above the
/// per-crate floor catches fleet-wide drift even when no single crate
/// individually fails.
pub const RECALL_THRESHOLD_TOTAL: f64 = 0.90;

/// Per-crate threshold dispatch.
///
/// Returns the recall floor for `crate_name`. The match-arm shape lets
/// callers override individual crates without introducing a side-channel
/// config file. Default arm returns [`RECALL_THRESHOLD_PER_CRATE`].
///
/// **Override discipline.** Adding a match arm is a reviewed PR — the
/// arm body cites the rationale (e.g. macro-heavy crate with audited
/// gaps). The match exists because future crates may legitimately need
/// a different floor, and we want that decision in source rather than
/// in an external file that drifts from the code.
// `#[allow(clippy::match_single_binding)]` is load-bearing: the match
// shape is the documented extension point (#340 AC-4 — "Per-crate
// threshold may differ from total via `match crate_name { ... }` inside
// the const-driven check"). Collapsing it to a bare expression would
// force the next per-crate override to re-introduce the match and
// change the call shape — that is exactly the kind of churn the issue's
// const-driven design avoids.
#[allow(clippy::match_single_binding)]
pub fn threshold_for_crate(crate_name: &str) -> f64 {
    match crate_name {
        // No per-crate overrides at the time of #340 landing. Future
        // overrides go here, each with a comment citing the issue or
        // RFC that motivated the deviation.
        _ => RECALL_THRESHOLD_PER_CRATE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pin the initial threshold values + assert percentage bounds in
    /// one test. Mirrors `tools/dogfood-enrich/src/thresholds.rs::tests`.
    /// A careless edit to either constant fails this test (not a silent
    /// ratchet); a typo setting one to 2.5 also fails on the bound.
    #[test]
    fn thresholds_pin_initial_values_and_are_valid_ratios() {
        assert_eq!(
            RECALL_THRESHOLD_PER_CRATE, 0.85,
            "RECALL_THRESHOLD_PER_CRATE initial floor moved"
        );
        assert_eq!(
            RECALL_THRESHOLD_TOTAL, 0.90,
            "RECALL_THRESHOLD_TOTAL initial floor moved"
        );
        for (name, value) in [
            ("RECALL_THRESHOLD_PER_CRATE", RECALL_THRESHOLD_PER_CRATE),
            ("RECALL_THRESHOLD_TOTAL", RECALL_THRESHOLD_TOTAL),
        ] {
            assert!(
                (0.0..=1.0).contains(&value),
                "{name} = {value} is not a valid ratio in [0.0, 1.0]"
            );
        }
        // Aggregate floor is at least as strict as the per-crate floor:
        // a fleet that passes per-crate but fails total is the bug we
        // want to catch (drift dispersed across many crates). Wrapped
        // in a `const` block so clippy treats this as a compile-time
        // assertion rather than a runtime tautology.
        const _: () = assert!(
            RECALL_THRESHOLD_TOTAL >= RECALL_THRESHOLD_PER_CRATE,
            "RECALL_THRESHOLD_TOTAL must be >= RECALL_THRESHOLD_PER_CRATE — \
             a looser total floor would let aggregate drift hide behind \
             per-crate passes"
        );
    }

    /// Default-arm dispatch returns the per-crate floor for any crate
    /// not explicitly overridden. Catches accidental deletions of the
    /// catch-all arm.
    #[test]
    fn threshold_for_crate_default_arm_returns_per_crate_floor() {
        for name in ["cfdb-core", "cfdb-extractor", "made-up-crate-name"] {
            assert_eq!(
                threshold_for_crate(name),
                RECALL_THRESHOLD_PER_CRATE,
                "threshold_for_crate({name:?}) should fall through to default"
            );
        }
    }
}
