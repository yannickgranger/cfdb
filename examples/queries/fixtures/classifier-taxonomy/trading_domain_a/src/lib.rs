//! `trading_domain_a` — fixture crate for the classifier taxonomy.
//!
//! Shapes produced (see fixture README):
//! - `OrderBook` is a DuplicatedFeature pair with `trading_domain_b::OrderBook`
//! - `Position::value` is a ContextHomonym pair with `portfolio_domain::Position::value`
//! - `OldSizer::compute` is #[deprecated] → UnfinishedRefactor
//! - `compute_qty_from_bps` + `compute_qty_from_pct` form a RandomScattering
//!   fork when both are reachable from the CLI entry point
//! - `Orphan::isolated` is CanonicalBypass (CANONICAL_FOR trading_concept but
//!   no callers from the CLI entry point)
//! - `dead_function` is Unwired (no caller reaches it)

/// Duplicated-feature pair: same struct name as `trading_domain_b::OrderBook`,
/// same bounded context (`trading`). Classifier emits a DuplicatedFeature row
/// per definition.
pub struct OrderBook {
    pub bids: Vec<u64>,
    pub asks: Vec<u64>,
}

/// Context-homonym pair: `Position::value` exists in both `trading_domain_a`
/// (returns f64) and `portfolio_domain` (returns u64). Divergent signatures
/// across distinct contexts → ContextHomonym finding.
pub struct Position {
    pub size: f64,
}

impl Position {
    pub fn value(&self, mark: f64) -> f64 {
        self.size * mark
    }
}

/// Unfinished-refactor: the `#[deprecated]` attribute is the v0.1 signal.
/// The classifier emits an UnfinishedRefactor row for the whole struct.
#[deprecated(note = "use NewSizer")]
pub struct OldSizer;

impl OldSizer {
    #[deprecated(note = "use NewSizer::compute")]
    pub fn compute(qty: u64) -> u64 {
        qty
    }
}

/// Random-scattering fork pair. Both resolvers share the concept prefix
/// `compute_qty` and diverge on suffix (`bps` vs `pct`). When both are
/// reachable from the CLI entry point (via `Dispatcher::dispatch`), the
/// classifier emits a RandomScattering row.
///
/// Method-based (not free fn) because HIR's `call_site_emitter` only
/// resolves `MethodCallExpr`, not `CallExpr`. Free fns produce syn-only
/// callsites (`callee_resolved: false`) with no CALLS edge, which
/// breaks reachability BFS. v0.3 gap: extend call_site_emitter to
/// handle CallExpr too.
pub struct Dispatcher;

impl Dispatcher {
    pub fn new() -> Self {
        Dispatcher
    }

    pub fn dispatch(&self, which: &str, balance: u64) -> u64 {
        if which == "bps" {
            self.compute_qty_from_bps(balance, 100)
        } else {
            self.compute_qty_from_pct(balance, 1)
        }
    }

    pub fn compute_qty_from_bps(&self, balance: u64, bps: u64) -> u64 {
        balance * bps / 10_000
    }

    pub fn compute_qty_from_pct(&self, balance: u64, pct: u64) -> u64 {
        balance * pct / 100
    }
}

/// Canonical-bypass signal: `Orphan::isolated` is CANONICAL_FOR the
/// `trading_concept` (by virtue of living in the canonical crate) but the
/// CLI entry point does not call it. `enrich_reachability` sets
/// `reachable_from_entry = false`; classifier-canonical-bypass.cypher
/// surfaces it.
pub struct Orphan;

impl Orphan {
    pub fn isolated() -> u64 {
        42
    }
}

/// Unwired signal: no caller anywhere in the workspace. `enrich_reachability`
/// leaves `reachable_from_entry = false`; classifier-unwired.cypher surfaces
/// it.
pub fn dead_function() -> u64 {
    7
}

