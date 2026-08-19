pub const RECALL_THRESHOLD_PER_CRATE: f64 = 0.85;

pub const RECALL_THRESHOLD_TOTAL: f64 = 0.90;

#[allow(clippy::match_single_binding)]
pub fn threshold_for_crate(crate_name: &str) -> f64 {
    match crate_name {
        _ => RECALL_THRESHOLD_PER_CRATE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        const _: () = assert!(
            RECALL_THRESHOLD_TOTAL >= RECALL_THRESHOLD_PER_CRATE,
            "RECALL_THRESHOLD_TOTAL must be >= RECALL_THRESHOLD_PER_CRATE — \
             a looser total floor would let aggregate drift hide behind \
             per-crate passes"
        );
    }

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
