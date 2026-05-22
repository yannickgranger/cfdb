//! `vsb_drop_fixture` — Pattern B `drop` kind ground-truth fixture.
//!
//! Reproduces qbot-core #2651 (compound-stop param drop) — the wire
//! form registers `stop_atr_mult`, but one of the two stop-policy
//! layers reads `active_mult` from the same config struct, and
//! `active_mult` is never wired through the REGISTERS_PARAM surface.
//!
//! The shape detected by `examples/queries/vertical-split-brain-drop.cypher`:
//!
//!   `:EntryPoint{Cli}` --REGISTERS_PARAM--> `:Field{name: "stop_atr_mult"}`
//!   `:EntryPoint{Cli}` --EXPOSES-----------> `:Item{Cli::handle}`
//!   `Cli::handle` --CALLS*--> `Engine::compute_active_mult`
//!     -- HAS_PARAM --> `:Param{name: "stop_atr_mult"}` ← matches wire
//!   `Cli::handle` --CALLS*--> `Engine::compute_trail_layer`
//!     -- HAS_PARAM --> `:Param{name: "active_mult"}`   ← divergent
//!     (and `active_mult` is NOT wire-registered → drop)
//!
//! The stand-in `Parser` trait (no `clap` dep) keeps the fixture's
//! compile time sub-second; `cfdb-hir-extractor`'s `entry_point_emitter`
//! detects `#[derive(Parser)]` syntactically (per the existing
//! `vsb_fixture` rationale).

#![allow(dead_code)]

pub trait Parser {}

/// Stand-in for clap's `#[arg(...)]` attribute. `cfdb-hir-extractor`'s
/// `field_has_arg_attr` matches by attribute-path last-segment (`arg`),
/// so a bare `#[arg]` on the field is enough to make the HIR side emit
/// `:EntryPoint -[:REGISTERS_PARAM]-> :Field` for it. Defining `arg` as
/// a no-op proc-macro-like helper isn't possible without a proc-macro
/// crate; using the `tool` shortcut isn't right either (that's for MCP
/// `#[tool]` fns). The minimal approach: bare `#[arg]` decoration that
/// we never actually evaluate, sidestepping the need for the `clap` dep.
///
/// rust-analyzer parses `#[arg]` as an attribute on the field (any
/// attribute path is accepted at parse time even if the macro doesn't
/// resolve), which is all the syntactic HIR scan needs.

/// Wire-form CLI. The `stop_atr_mult` field is the wire-registered
/// param key — `cfdb-hir-extractor` emits
/// `:EntryPoint -[:REGISTERS_PARAM]-> :Field{name: "stop_atr_mult"}`
/// for it (per `field_has_arg_attr` keying on the `#[arg]` attribute
/// path).
#[derive(Parser)]
pub struct Cli {
    #[arg]
    pub stop_atr_mult: f64,
}

impl Cli {
    /// The handler reaches BOTH stop-policy layers via the engine.
    /// In real qbot-core this is the `compound_stop` dispatcher that
    /// routes to active-multiplier and trailing-multiplier in turn —
    /// the bug is that the trailing layer ignores the wire key and
    /// reads `active_mult` instead.
    pub fn handle(&self, engine: &Engine) {
        engine.dispatch(self.stop_atr_mult);
    }
}

pub struct Engine;

impl Engine {
    /// Compound-stop dispatcher. Reachable from `Cli::handle`; calls
    /// BOTH stop-policy layers so `:Item -[:CALLS*]->` reaches both
    /// resolvers from the entry point.
    pub fn dispatch(&self, mult: f64) {
        let _active = compute_active_mult(mult);
        let _trail = compute_trail_layer(mult);
    }
}

/// Layer-K resolver. Its parameter `:Param.name = "stop_atr_mult"`
/// matches the wire key — the cypher rule's `matched.name = wire.name`
/// branch binds here.
pub fn compute_active_mult(stop_atr_mult: f64) -> f64 {
    stop_atr_mult
}

/// Layer-K+1 resolver — the **drop**. Its parameter
/// `:Param.name = "active_mult"` does NOT match the wire key
/// `stop_atr_mult`. The rule's `divergent.name <> wire.name` branch
/// binds here, and the `NOT EXISTS { ep -[:REGISTERS_PARAM]-> other
/// WHERE other.name = "active_mult" }` confirms `active_mult` is
/// never wired — the drop is real.
pub fn compute_trail_layer(active_mult: f64) -> f64 {
    active_mult
}
