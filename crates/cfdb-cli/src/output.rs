//! Tiny output helpers — keep stdout shape consistent across handlers.

use serde::Serialize;

use crate::CfdbCliError;

/// Pretty-print `payload` as JSON to stdout (newline-terminated via println!).
/// Centralises the `serde_json::to_string_pretty + println!` shape that every
/// JSON-emitting handler used to inline. Reachable from the binary crate
/// (`main_dispatch.rs`) via the crate-root `pub use` re-export, same pattern
/// as the other handler exports.
pub fn emit_json<T: Serialize + ?Sized>(payload: &T) -> Result<(), CfdbCliError> {
    let json = serde_json::to_string_pretty(payload)?;
    println!("{json}");
    Ok(())
}
