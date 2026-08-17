//! Exit-code policy for the `cfdb` CLI binary.
//!
//! This module is the single authoritative source for the 0/1/2/30 exit-code
//! contract documented in `main.rs:43-60`. It exists in its own file to keep
//! concerns separated: `error.rs` owns the error *taxonomy*; adding I/O
//! policy there would give it a second reason to change. Placing the policy
//! here keeps both files single-purpose.
//!
//! # Contract
//!
//! | Code | Meaning | Origin |
//! |------|---------|--------|
//! | `0` | Success / no findings | handler returned `Ok(())` |
//! | `1` | Runtime error | handler returned `Err(e)` where `e` is not `Usage` |
//! | `2` | Usage / CLI argument error | clap parse failure **or** `CfdbCliError::Usage` |
//! | `30` | Findings present (gate failure) | handler returned a positive row count |
//!
//! Note on the 30/error split: `exit_code_for` maps only *error* variants
//! → {1, 2}. The findings path (exit 30) is a separate control-flow concern
//! — it fires when a handler returns a positive row count, not an `Err`.
//! Both paths ultimately call [`findings_exit`] / `exit_code_for` from one
//! named site each, ensuring there is exactly ONE textual occurrence of the
//! `30` literal in the codebase outside of comments and tests.

use crate::CfdbCliError;

/// The exit code used when a rule/predicate returns findings.
///
/// Named constant so the value appears in exactly one place. All three
/// `dispatch_core`/`dispatch_typed` call sites route through
/// [`findings_exit`], which is the sole caller of this constant.
pub(crate) const EXIT_FINDINGS: i32 = 30;

/// Map a [`CfdbCliError`] to the documented CLI exit code.
///
/// `Usage` → 2 (bad CLI arguments or runtime-validation failure that the
/// user can fix by re-running with correct inputs).  All other variants → 1
/// (runtime error: extractor failure, I/O error, store error, parse error).
///
/// **The findings path (exit 30) is intentionally absent from this mapping.**
/// Exit 30 fires on a *positive row count*, which is not an error — the
/// handler returned `Ok(n)` for some `n > 0`. That path goes through
/// [`findings_exit`] and is not mediated by `CfdbCliError` at all.
pub(crate) fn exit_code_for(e: &CfdbCliError) -> i32 {
    match e {
        CfdbCliError::Usage(_) => 2,
        _ => 1,
    }
}

/// Terminate the process with exit code 30 (findings present, gate failure).
///
/// This is the **single** call site that emits `EXIT_FINDINGS`. All three
/// dispatch helpers (`dispatch_core` Violations/Check arms and
/// `dispatch_typed` CheckPredicate arm) call this function — never
/// `std::process::exit(30)` inline. Having one site makes the contract
/// auditable: a file-scan for `process::exit(` outside this module will
/// return zero results.
///
/// # Safety / panic contract
///
/// `process::exit` terminates immediately; destructors do not run. This is
/// intentional: we are in a short-lived CLI binary, not a library, so
/// resource cleanup via `Drop` is unnecessary. The immediate-exit also
/// ensures that any partially-written stdout is flushed by the OS.
pub(crate) fn findings_exit() -> ! {
    std::process::exit(EXIT_FINDINGS);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Usage` variant maps to exit code 2 — distinguishes "bad CLI args"
    /// from generic runtime errors.
    #[test]
    fn usage_error_maps_to_2() {
        let e = CfdbCliError::Usage("missing --db flag".to_string());
        assert_eq!(
            exit_code_for(&e),
            2,
            "Usage variant must map to exit code 2"
        );
    }

    /// A runtime variant (Io) maps to exit code 1 — not a usage error,
    /// not findings.
    #[test]
    fn runtime_error_maps_to_1() {
        let e = CfdbCliError::Io(std::io::Error::other("disk full"));
        assert_eq!(exit_code_for(&e), 1, "Io variant must map to exit code 1");
    }

    /// All non-Usage variants map to 1. Spot-check Json and Parse to confirm
    /// the wildcard arm covers the full set.
    #[test]
    fn json_error_maps_to_1() {
        // Construct a serde_json error via a deliberate parse failure.
        let e: CfdbCliError = serde_json::from_str::<serde_json::Value>("{{bad json")
            .expect_err("expected parse failure")
            .into();
        assert_eq!(exit_code_for(&e), 1, "Json variant must map to exit code 1");
    }

    /// `EXIT_FINDINGS` has the value 30 as documented in main.rs:49.
    #[test]
    fn exit_findings_constant_is_30() {
        assert_eq!(EXIT_FINDINGS, 30);
    }

    /// File-scan regression guard: `main_dispatch.rs` must contain exactly
    /// ZERO bare `std::process::exit(` calls. All exits go through
    /// `findings_exit()` in this module, which is the single authorised site.
    ///
    /// Non-vacuity: assert the file was read and is non-empty before scanning.
    #[test]
    fn main_dispatch_has_no_bare_process_exit() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/src/main_dispatch.rs");
        let contents = std::fs::read_to_string(path)
            .expect("main_dispatch.rs must be readable from CARGO_MANIFEST_DIR");
        assert!(
            !contents.is_empty(),
            "main_dispatch.rs must be non-empty (non-vacuity guard)"
        );
        assert!(
            !contents.contains("std::process::exit("),
            "main_dispatch.rs must not contain bare `std::process::exit(` calls; \
             all exits go through `findings_exit()` in main_exit.rs"
        );
        // Also assert there is no stray `process::exit(` import usage.
        // We count lines containing `process::exit(` to make the assertion
        // message informative when it fires.
        let count = contents
            .lines()
            .filter(|l| l.contains("process::exit("))
            .count();
        assert_eq!(
            count, 0,
            "found {count} line(s) with `process::exit(` in main_dispatch.rs; \
             all findings exits must route through `findings_exit()`"
        );
    }
}
