//! Scar — CLI entry point reaches two `CompoundStop` constructors
//! through a 2-hop chain (`handle → build_stop_config → {new | from_legacy}`).
//!
//! Both `CompoundStop::new` and `CompoundStop::from_legacy` return
//! `CompoundStop`. The mid-layer `build_stop_config` dispatches on a
//! legacy-flag — the failure mode is that callers register one
//! compound-stop concept but the resolver layer has two variants.
//!
//! `vsb-multi-resolver.cypher` MUST flag this handler with the 2-hop
//! reach:
//!   entry_point   = "run-stop"
//!   param_name    = "compound_stop"
//!   param_type    = "compound_stop_layer_isolation_scar::CompoundStop"
//!   resolvers     = [CompoundStop::from_legacy, CompoundStop::new]

pub struct CompoundStop {
    pub initial_atr: f64,
    pub trail_mult: f64,
}

impl CompoundStop {
    pub fn new(initial_atr: f64, trail_mult: f64) -> Self {
        Self {
            initial_atr,
            trail_mult,
        }
    }

    /// Legacy constructor kept for config-file back-compat. Produces
    /// the same type via a different path — the split the detector
    /// flags.
    pub fn from_legacy(bps_basis: f64) -> Self {
        Self {
            initial_atr: bps_basis / 100.0,
            trail_mult: 2.0,
        }
    }
}

pub mod cli {
    use super::CompoundStop;

    pub fn build_stop_config(atr: f64, mult: f64, legacy: bool) -> CompoundStop {
        if legacy {
            CompoundStop::from_legacy(atr * 100.0)
        } else {
            CompoundStop::new(atr, mult)
        }
    }

    pub struct RunStop {
        pub compound_stop: CompoundStop,
    }

    impl RunStop {
        pub fn handle(_compound_stop: &str) {
            let _ = build_stop_config(1.0, 2.0, false);
        }
    }
}
