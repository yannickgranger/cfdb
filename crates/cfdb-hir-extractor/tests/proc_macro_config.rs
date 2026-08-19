use ra_ap_load_cargo::ProcMacroServerChoice;

use cfdb_hir_extractor::hir_db::{
    build_load_config, build_load_config_with_probe, proc_macro_server_available,
};

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

#[test]
fn unit_b_proc_macros_true_with_probe_true_yields_sysroot_one() {
    let config = build_load_config_with_probe(true, || true);
    assert!(matches!(
        config.with_proc_macro_server,
        ProcMacroServerChoice::Sysroot
    ));
    assert_eq!(config.proc_macro_processes, 1);
}

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

#[test]
fn unit_c_proc_macros_true_with_probe_false_yields_silent_fallback() {
    let config = build_load_config_with_probe(true, || false);
    assert!(matches!(
        config.with_proc_macro_server,
        ProcMacroServerChoice::None
    ));
    assert_eq!(config.proc_macro_processes, 0);
}

#[test]
fn unit_e_vfs_virtual_path_excluded_from_walk() {
    use ra_ap_vfs::VfsPath;
    let virtual_path = VfsPath::new_virtual_path("/macro-expanded/synthetic.rs".to_string());
    assert!(
        virtual_path.as_path().is_none(),
        "VfsPath::Virtual MUST NOT convert to a PathBuf — \
         call_site_emitter.rs:113-114 relies on this to skip macro-expanded virtual files"
    );
}

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
