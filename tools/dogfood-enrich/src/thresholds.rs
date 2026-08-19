pub const DEPRECATION_THRESHOLD: Option<u32> = None;

pub const RFC_DOCS_THRESHOLD: Option<u32> = None;

pub const BC_COVERAGE_THRESHOLD: Option<u32> = Some(95);

pub const CONCEPTS_THRESHOLD: Option<u32> = None;

pub const REACHABILITY_THRESHOLD: Option<u32> = Some(80);

pub const METRICS_COVERAGE_THRESHOLD: Option<u32> = Some(95);

pub const GIT_COVERAGE_THRESHOLD: Option<u32> = Some(95);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thresholds_pin_initial_values_and_are_valid_percentages() {
        for (name, value) in [
            ("DEPRECATION_THRESHOLD", DEPRECATION_THRESHOLD),
            ("RFC_DOCS_THRESHOLD", RFC_DOCS_THRESHOLD),
            ("CONCEPTS_THRESHOLD", CONCEPTS_THRESHOLD),
        ] {
            assert!(
                value.is_none(),
                "{name} should be None (hard-equality sentinel) but is {value:?}"
            );
        }
        for (name, value, expected) in [
            ("BC_COVERAGE_THRESHOLD", BC_COVERAGE_THRESHOLD, 95),
            ("REACHABILITY_THRESHOLD", REACHABILITY_THRESHOLD, 80),
            ("METRICS_COVERAGE_THRESHOLD", METRICS_COVERAGE_THRESHOLD, 95),
            ("GIT_COVERAGE_THRESHOLD", GIT_COVERAGE_THRESHOLD, 95),
        ] {
            let v = value.expect("ratio threshold must be Some");
            assert_eq!(v, expected, "{name} initial floor moved");
            assert!(v <= 100, "{name} = {v} is not a valid percentage");
        }
    }
}
