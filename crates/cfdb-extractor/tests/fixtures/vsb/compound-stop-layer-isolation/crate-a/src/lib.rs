//! Clean baseline — CLI `run-stop` command registers a single
//! `CompoundStop` param and the reachable call chain has exactly one
//! resolver returning `CompoundStop` (`CompoundStop::new`).

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
}

pub mod cli {
    use super::CompoundStop;

    pub fn build_stop_config(atr: f64, mult: f64) -> CompoundStop {
        CompoundStop::new(atr, mult)
    }

    pub struct RunStop {
        pub compound_stop: CompoundStop,
    }

    impl RunStop {
        pub fn handle(_compound_stop: &str) {
            let _ = build_stop_config(1.0, 2.0);
        }
    }
}
