//! MCP tool fixtures (Issue #126 v0.2-1 coverage gate).
//!
//! Two shapes:
//! - Bare `#[tool]` attribute.
//! - Fully-qualified `#[rmcp::tool]` attribute.
//!
//! The extractor's detector (`has_tool_attr` in
//! `entry_point_emitter.rs`) matches the last path segment of the
//! attribute meta path, so `#[tool]`, `#[tool(...)]`, `#[rmcp::tool]`,
//! and `#[foo::bar::tool]` all fire uniformly.

/// First MCP tool — bare `#[tool]` attribute.
#[tool]
pub fn echo(input: &str) -> String {
    input.to_string()
}

/// Second MCP tool — namespaced `#[rmcp::tool]` attribute. The scan
/// walks the meta path and grabs the last segment, so this fires.
#[rmcp::tool]
pub fn ping() -> &'static str {
    "pong"
}

/// Control fn — no `#[tool]` attribute, must NOT be emitted.
pub fn unrelated_helper() -> u32 {
    42
}
