//! `vsb_fixture` — Pattern B vertical-split-brain ground-truth fixture.
//!
//! Reproduces the qbot-core #2651 compound-stop resolver-fork shape: one
//! CLI entry point (`Cli`) reaches an `Engine` which in turn calls two
//! `StopLoss` resolvers with divergent input types. Both resolvers are
//! on the reachable call chain from the single entry point — the exact
//! signal `vertical-split-brain.cypher` targets.
//!
//! The stand-in `Parser` trait exists purely to satisfy the HIR
//! extractor's syntactic `#[derive(Parser)]` scan (`entry_point_emitter`
//! matches on attribute text, not on the real clap trait). Real
//! consumers would depend on `clap` — the fixture pulls zero external
//! crates on purpose so `build_hir_database` stays in the sub-second
//! range per test.

#![allow(dead_code)]

pub trait Parser {}

#[derive(Parser)]
pub struct Cli {
    pub raw: RawStop,
}

impl Cli {
    pub fn handle(&self, engine: &Engine) {
        engine.build_stop(&self.raw);
    }
}

/// The user-supplied shape. Carries BOTH a bps field and a pct field,
/// which is precisely the failure mode `#2651` hit: when the CLI form
/// and the MCP form populated different subsets, one resolver saw a
/// zero default it was never meant to.
pub struct RawStop {
    pub bps: u32,
    pub pct: u32,
}

pub struct Engine;

impl Engine {
    /// Wires BOTH resolvers on the reachable chain. This is the
    /// "split-brain" shape — a later session changes `stop_from_bps`
    /// and the chain through `stop_from_pct` silently keeps the old
    /// behaviour until the two diverge enough to trigger the gate.
    ///
    /// Resolver names carry the concept prefix `stop_` so the v0.2
    /// MVP rule (which keys on `^(\w+)_(from|to|for|as)_(\w+)$` —
    /// non-empty prefix required) fires on this shape. The shape
    /// matches the qbot-core idiom where constructors encode the
    /// domain concept in the method name rather than relying on the
    /// enclosing type to disambiguate.
    pub fn build_stop(&self, raw: &RawStop) -> StopLoss {
        let via_bps = StopLoss::stop_from_bps(raw.bps);
        let via_pct = StopLoss::stop_from_pct(raw.pct);
        // In real code one would be chosen over the other; the fixture
        // uses both return values so the Rust compiler does not warn
        // and (more importantly) so both CALLS edges definitely land
        // in the resolved set.
        if via_bps.value > via_pct.value {
            via_bps
        } else {
            via_pct
        }
    }
}

pub struct StopLoss {
    pub value: u64,
}

impl StopLoss {
    /// Resolver A — accepts basis points. Named with the `stop_`
    /// concept prefix so the MVP cypher rule's name-shape heuristic
    /// fires (see crate-root docs).
    pub fn stop_from_bps(bps: u32) -> StopLoss {
        StopLoss {
            value: u64::from(bps),
        }
    }

    /// Resolver B — accepts whole percent.
    pub fn stop_from_pct(pct: u32) -> StopLoss {
        StopLoss {
            value: u64::from(pct) * 100,
        }
    }
}
