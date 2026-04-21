//! `trading_cli` — registers a `:EntryPoint` so the reachability
//! enrichment has a seed. Entry point is a `#[tool]`-attributed fn
//! (NOT a clap derive struct) because the `cli_command` EntryPoint's
//! EXPOSES target IS the struct, which has no outgoing CALLS edges —
//! reachability BFS cannot seed from a struct. The `mcp_tool` kind
//! targets a fn directly, producing the CALLS chain needed for the
//! RandomScattering (Pattern B fork) classifier input. The scar is
//! about resolver fork shape in a single bounded context, not the
//! entry-point kind.

use clap::{Parser, Subcommand};

#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    Trade { balance: u64, which: String },
}

#[tool]
fn run(balance: u64, which: &str) {
    // Method call (not free fn) so HIR's `call_site_emitter` emits the
    // resolved CallSite + CALLS edge → reachability BFS propagates.
    let d = trading_domain_a::Dispatcher::new();
    let qty = d.dispatch(which, balance);
    println!("{qty}");
}

fn main() {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Trade { balance, which } => run(balance, &which),
    }
}
