//! HTTP route fixtures (Issue #126 v0.2-1 coverage gate).
//!
//! Two shapes:
//! - axum `.route("/p", handler)` on a `Router` chain.
//! - actix `App::new().service(web::resource("/p").route(web::get().to(h)))`.
//!
//! Stand-ins mirror the real `axum::Router` / `actix_web::{App, web}`
//! surface at method-signature granularity — the extractor is purely
//! syntactic on the call chain.

// ---- axum stand-ins ---------------------------------------------------

pub struct Router;
impl Router {
    pub fn new() -> Self {
        Router
    }
    pub fn route<H>(self, _path: &str, _handler: H) -> Self {
        self
    }
    pub fn get<H>(self, _path: &str, _handler: H) -> Self {
        self
    }
}

/// First http_route handler — axum `.route("/users", list_users)`.
pub fn list_users() {}

/// Second http_route handler — axum `.get("/users/:id", show_user)`.
/// Uses the `get` convenience method to exercise a different dispatch
/// arm in the scanner (still 2-arg `/path, handler` shape).
pub fn show_user() {}

/// Builder showcasing both axum shapes in a single chain.
pub fn build_axum() -> Router {
    Router::new()
        .route("/users", list_users)
        .get("/users/:id", show_user)
}

// ---- actix stand-ins --------------------------------------------------

pub struct App;
impl App {
    pub fn new() -> Self {
        App
    }
    pub fn service(self, _svc: Resource) -> Self {
        self
    }
}
pub struct Resource;
impl Resource {
    pub fn route(self, _route: Route) -> Resource {
        self
    }
}
pub struct Route;
impl Route {
    pub fn to<H>(self, _handler: H) -> Route {
        self
    }
}
pub mod web {
    pub fn resource(_path: &str) -> super::Resource {
        super::Resource
    }
    pub fn get() -> super::Route {
        super::Route
    }
}

/// Third http_route handler — actix
/// `web::resource("/health").route(web::get().to(health))`.
pub fn health() {}

/// Builder showcasing the actix resource-chain shape — the path
/// literal lives on the `web::resource` receiver, not on the `.route`
/// / `.to` call. Exercises `receiver_resource_path` in the scanner.
pub fn build_actix() -> App {
    App::new().service(web::resource("/health").route(web::get().to(health)))
}

/// Control fn — must NOT be emitted (never passed to any route
/// registration).
pub fn unrelated_handler() {}
