use crate::CfdbCliError;

pub(crate) const EXIT_FINDINGS: i32 = 30;

pub(crate) fn exit_code_for(e: &CfdbCliError) -> i32 {
    match e {
        CfdbCliError::Usage(_) => 2,
        _ => 1,
    }
}

pub(crate) fn findings_exit() -> ! {
    std::process::exit(EXIT_FINDINGS);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_error_maps_to_2() {
        let e = CfdbCliError::Usage("missing --db flag".to_string());
        assert_eq!(
            exit_code_for(&e),
            2,
            "Usage variant must map to exit code 2"
        );
    }

    #[test]
    fn runtime_error_maps_to_1() {
        let e = CfdbCliError::Io(std::io::Error::other("disk full"));
        assert_eq!(exit_code_for(&e), 1, "Io variant must map to exit code 1");
    }

    #[test]
    fn json_error_maps_to_1() {
        let e: CfdbCliError = serde_json::from_str::<serde_json::Value>("{{bad json")
            .expect_err("expected parse failure")
            .into();
        assert_eq!(exit_code_for(&e), 1, "Json variant must map to exit code 1");
    }

    #[test]
    fn exit_findings_constant_is_30() {
        assert_eq!(EXIT_FINDINGS, 30);
    }

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
