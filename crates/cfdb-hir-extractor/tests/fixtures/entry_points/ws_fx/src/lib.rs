//! Websocket fixtures (Issue #126 v0.2-1 coverage gate).
//!
//! Two shapes:
//! - `ws.on_upgrade(named_handler)` — handler is a named fn; EXPOSES
//!   that fn's qname.
//! - `ws.on_upgrade(|_socket| { ... })` — closure handler; EXPOSES
//!   the enclosing fn (closure has no path-level qname).
//!
//! Stand-ins mirror `axum::extract::ws::{WebSocket, WebSocketUpgrade}`
//! at method-signature granularity.

pub struct WebSocket;
pub struct WebSocketUpgrade;
impl WebSocketUpgrade {
    pub fn on_upgrade<F>(self, _f: F) -> Response
    where
        F: FnOnce(WebSocket),
    {
        Response
    }
}
pub struct Response;

/// Named websocket handler — EXPOSES target for `mount_named`.
pub fn chat_handler(_socket: WebSocket) {}

/// First websocket entry point — `on_upgrade(named_fn)`. EXPOSES
/// `ws_fx::chat_handler` (resolved via path lookup).
pub fn mount_named(upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(chat_handler)
}

/// Second websocket entry point — `on_upgrade(|socket| { ... })`.
/// Closure has no qname; EXPOSES the enclosing `mount_inline` fn.
pub fn mount_inline(upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(|_socket| {})
}

/// Control fn — no `.on_upgrade(...)` call, must NOT be emitted.
pub fn unrelated_ws_helper() {}
