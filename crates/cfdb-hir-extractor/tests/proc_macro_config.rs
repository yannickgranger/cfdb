//! RFC-043 issue #418 — prescribed `Tests:` block (a), (b), (c), (e), (f).
//! Test (d) — `cfdb extract --no-proc-macro` CLI round-trip — lives in
//! `crates/cfdb-cli/tests/extract_args_no_proc_macro.rs` because the clap
//! parsing surface is owned by the CLI crate.
//!
//! Tests (a) and (c) are pure unit tests on `build_load_config_with_probe`,
//! the dependency-injectable variant that takes a probe closure rather than
//! reading the active sysroot. Test (b) is a real-sysroot test gated behind
//! `#[ignore]` because not every CI image ships `rust-analyzer-proc-macro-srv`.
//! Test (e) exercises the VfsPath::Virtual exclusion implicit in
//! `vfs_path_to_pathbuf`. Test (f) asserts the schema descriptor text
//! carries the §4 I6 sentences.

use ra_ap_load_cargo::ProcMacroServerChoice;

use cfdb_hir_extractor::hir_db::{
    build_load_config, build_load_config_with_probe, proc_macro_server_available,
};

/// (a) `build_hir_database` with `proc_macros=false` produces
/// `LoadCargoConfig{with_proc_macro_server: None, proc_macro_processes: 0}`.
/// The probe is NEVER consulted in this path — operator-explicit disable
/// short-circuits before any sysroot check.
#[test]
fn unit_a_proc_macros_false_yields_none_zero_no_probe() {
    let probe_called = std::cell::Cell::new(false);
    let config = build_load_config_with_probe(false, || {
        probe_called.set(true);
        true
    });
    assert!(matches!(
        config.with_proc_macro_server,
        ProcMacroServerChoice::None
    ));
    assert_eq!(config.proc_macro_processes, 0);
    assert!(
        !probe_called.get(),
        "probe must not run when proc_macros=false"
    );
}

/// (b) `build_load_config` with `proc_macros=true` on a real sysroot
/// where the probe returns true produces
/// `LoadCargoConfig{with_proc_macro_server: Sysroot, proc_macro_processes: 1}`.
///
/// Uses the dependency-injected probe form so the test is sysroot-
/// independent. A second sibling test (`unit_b_real_sysroot_smoke`)
/// runs against the live sysroot and is `#[ignore]`d by default.
#[test]
fn unit_b_proc_macros_true_with_probe_true_yields_sysroot_one() {
    let config = build_load_config_with_probe(true, || true);
    assert!(matches!(
        config.with_proc_macro_server,
        ProcMacroServerChoice::Sysroot
    ));
    assert_eq!(config.proc_macro_processes, 1);
}

/// (b smoke) The default `build_load_config` honors the real sysroot.
/// Ignored by default because not every CI image ships
/// `rust-analyzer-proc-macro-srv`. Run explicitly with
/// `cargo test -p cfdb-hir-extractor unit_b_real_sysroot_smoke -- --ignored`
/// on a rustup-managed toolchain.
#[test]
#[ignore = "requires rust-analyzer-proc-macro-srv in the active sysroot; run --ignored on rustup hosts"]
fn unit_b_real_sysroot_smoke() {
    assert!(
        proc_macro_server_available(),
        "expected `rust-analyzer-proc-macro-srv` in active sysroot for this smoke test; \
         install via `rustup component add rust-analyzer`"
    );
    let config = build_load_config(true);
    assert!(matches!(
        config.with_proc_macro_server,
        ProcMacroServerChoice::Sysroot
    ));
    assert_eq!(config.proc_macro_processes, 1);
}

/// (c) `build_load_config_with_probe` with `proc_macros=true` and a
/// probe that returns false (stub sysroot WITHOUT
/// `rust-analyzer-proc-macro-srv`) silently falls back to
/// `LoadCargoConfig{with_proc_macro_server: None, proc_macro_processes: 0}`.
/// Per RFC-043 §3.3 case 1: NO `Err` is returned; a stderr warning is
/// emitted (visually verifiable via cargo test stderr — automated stderr
/// capture is overkill for one line and would couple the test to
/// `eprintln!`'s implementation).
#[test]
fn unit_c_proc_macros_true_with_probe_false_yields_silent_fallback() {
    let config = build_load_config_with_probe(true, || false);
    // Silent fallback — config matches the proc_macros=false case
    // exactly. The two CANNOT diverge per solid-architect R2 CR1's
    // `pm_enabled` invariant.
    assert!(matches!(
        config.with_proc_macro_server,
        ProcMacroServerChoice::None
    ));
    assert_eq!(config.proc_macro_processes, 0);
}

/// (e) VfsPath::Virtual filter assertion — `vfs_path_to_pathbuf` at
/// `crates/cfdb-hir-extractor/src/call_site_emitter.rs:113-114` filters
/// out virtual-path entries (macro-expanded files injected by ra_ap_hir).
/// This is the protection ddd-specialist R2 named: macro-expansion
/// virtual paths cannot produce phantom `:CallSite` nodes because the
/// walk skips them at filename-conversion time.
///
/// We assert the contract via the public API: a `VfsPath::new_virtual_path`
/// value cannot be converted to a `PathBuf` (the conversion only succeeds
/// for `VfsPath::PathBuf`). The filter at call_site_emitter.rs is then
/// just `Option`-based exclusion of the None case.
#[test]
fn unit_e_vfs_virtual_path_excluded_from_walk() {
    use ra_ap_vfs::VfsPath;
    let virtual_path = VfsPath::new_virtual_path("/macro-expanded/synthetic.rs".to_string());
    // The conversion to filesystem PathBuf is the gate. `as_path()`
    // returns Some only for real-disk paths.
    assert!(
        virtual_path.as_path().is_none(),
        "VfsPath::Virtual MUST NOT convert to a PathBuf — \
         call_site_emitter.rs:113-114 relies on this to skip macro-expanded virtual files"
    );
}

/// (f) The `:CallSite.callee_resolved` schema descriptor includes the
/// two §4 I6 sentences: (1) the epistemic-precision shift and (2) the
/// silent-fallback indistinguishability. ddd-specialist R2 implementer
/// note.
#[test]
fn unit_f_callee_resolved_descriptor_carries_rfc043_caveats() {
    use cfdb_core::schema::schema_describe;

    let describe = schema_describe();
    let call_site_node = describe
        .nodes
        .iter()
        .find(|n| n.label.as_str() == "CallSite")
        .expect("CallSite descriptor must exist");
    let callee_resolved = call_site_node
        .attributes
        .iter()
        .find(|a| a.name == "callee_resolved")
        .expect("CallSite.callee_resolved attribute must exist");

    assert!(
        callee_resolved.description.contains("RFC-043"),
        "callee_resolved descriptor must reference RFC-043 (epistemic-precision sentence); got:\n{}",
        callee_resolved.description
    );
    assert!(
        callee_resolved
            .description
            .contains("silent probe fallback")
            || callee_resolved
                .description
                .contains("indistinguishable from `--no-proc-macro`"),
        "callee_resolved descriptor must note the silent-fallback indistinguishability; got:\n{}",
        callee_resolved.description
    );
}
