// Canonical-bypass fixture — MCP-tool `:EntryPoint` anchor.
//
// The `record` fn carries `#[tool]` — the HIR extractor emits a
// `:EntryPoint { kind: "mcp_tool" }` targeting this fn directly. The
// reachability BFS seeds from `:EntryPoint`-EXPOSES-target and walks
// outward via CALLS + INVOKES_AT, so everything `record` calls is
// `reachable_from_entry = true`.
//
// Entry-point kind choice: earlier iterations used `#[derive(Parser)]`
// on a `Cli` struct, but the `cli_command` entry point's EXPOSES target
// IS the struct, which has no outgoing CALLS edges. That made every
// service method unreachable and every verdict degenerate. `mcp_tool`
// targets a fn directly, which is the shape the reachability BFS
// requires. Scar coverage (BYPASS_REACHABLE / CANONICAL_CALLER
// vs. BYPASS_DEAD / CANONICAL_UNREACHABLE) is unchanged by the
// entry-point-kind swap — the scar is about service-layer canonical
// bypass (#3525), not entry-point shape.
//
// The attribute stand-in is the bare `#[tool]` token — the HIR scan is
// textual on attribute identifiers (last path segment == "tool"), so
// no real mcp / rmcp dep is needed.

use ledger::{LedgerRepository, LedgerService};

pub struct RealRepo;
impl LedgerRepository for RealRepo {
    fn append(&self, _entries: Vec<i64>) {}
    fn append_idempotent(&self, _r: &str, _e: Vec<i64>) {}
}

// MCP-tool entry point. Calls exactly two service methods:
//   - record_trade       → BYPASS_REACHABLE
//   - record_trade_safe  → CANONICAL_CALLER
//
// `record_orphan` and `record_isolated` have no reachable caller, so
// reachability BFS leaves them `reachable_from_entry = false`. They
// surface as BYPASS_DEAD and CANONICAL_UNREACHABLE respectively.
#[tool]
pub fn record(svc: &LedgerService<RealRepo>) {
    svc.record_trade();
    svc.record_trade_safe();
}
