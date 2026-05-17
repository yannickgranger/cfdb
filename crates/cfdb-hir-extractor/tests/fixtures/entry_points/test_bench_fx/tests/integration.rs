//! Integration-test target — exercises attribute-driven test detection
//! (`#[test]`, `#[tokio::test]`) plus file-location fallback for an
//! unattributed helper fn AND precedence of `#[tool]` over file location.

/// Bare `#[test]` attribute — must emit `:EntryPoint{kind="test"}`.
#[test]
fn test_plain() {}

/// `#[tokio::test]` — last path segment is `test`; must emit
/// `:EntryPoint{kind="test"}`.
#[tokio::test]
async fn test_tokio() {}

/// No attribute, but lives under `tests/` — file-location detection
/// must emit `:EntryPoint{kind="test"}`.
fn test_helper() {}

/// `#[tool]` inside `tests/` — precedence rule (RFC-042 §3.1 item 1):
/// `#[tool]` wins, kind is `mcp_tool`, NOT `test`.
#[tool]
fn helper_with_tool() {}
