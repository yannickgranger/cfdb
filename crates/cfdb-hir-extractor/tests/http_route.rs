use std::fs;
use std::path::Path;

use cfdb_core::fact::{Node, PropValue};
use cfdb_core::qname::item_node_id;
use cfdb_core::schema::{EdgeLabel, Label};
use cfdb_hir_extractor::{build_hir_database, extract_entry_points};
use tempfile::tempdir;

fn write(root: &Path, rel: &str, contents: &str) {
    let p = root.join(rel);
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).expect("fixture mkdir -p");
    }
    fs::write(p, contents).expect("fixture write");
}

fn write_workspace(root: &Path, lib_rs: &str) {
    write(
        root,
        "Cargo.toml",
        r#"[workspace]
resolver = "2"
members = ["routes"]
"#,
    );
    write(
        root,
        "routes/Cargo.toml",
        r#"[package]
name = "routes"
version = "0.0.1"
edition = "2021"

[dependencies]
"#,
    );
    write(root, "routes/src/lib.rs", lib_rs);
}

fn write_workspace_with_dep(root: &Path, lib_rs: &str, dep: &str) {
    write(
        root,
        "Cargo.toml",
        &format!("[workspace]\nresolver = \"2\"\nmembers = [\"routes\", \"{dep}\"]\n"),
    );
    write(
        root,
        "routes/Cargo.toml",
        &format!(
            "[package]\nname = \"routes\"\nversion = \"0.0.1\"\nedition = \"2021\"\n\n\
             [dependencies]\n{dep} = {{ path = \"../{dep}\" }}\n"
        ),
    );
    write(root, "routes/src/lib.rs", lib_rs);
    write(
        root,
        &format!("{dep}/Cargo.toml"),
        &format!(
            "[package]\nname = \"{dep}\"\nversion = \"0.0.1\"\nedition = \"2021\"\n\n[dependencies]\n"
        ),
    );
    write(root, &format!("{dep}/src/lib.rs"), "");
}

fn http_routes(root: &Path) -> (Vec<Node>, Vec<cfdb_core::fact::Edge>) {
    let (db, vfs, _pm_client, targets) =
        build_hir_database(root, false).expect("build_hir_database");
    let (nodes, edges) =
        extract_entry_points(&db, &vfs, root, &targets).expect("extract_entry_points");
    let http = nodes
        .into_iter()
        .filter(|n| {
            n.label.as_str() == Label::ENTRY_POINT
                && n.props.get("kind").and_then(PropValue::as_str) == Some("http_route")
        })
        .collect::<Vec<_>>();
    let exposes = edges
        .into_iter()
        .filter(|e| e.label.as_str() == EdgeLabel::EXPOSES)
        .collect::<Vec<_>>();
    (http, exposes)
}

fn assert_one_route(
    nodes: &[Node],
    edges: &[cfdb_core::fact::Edge],
    expected_path: &str,
    expected_handler_qname: &str,
) {
    assert_eq!(
        nodes.len(),
        1,
        "expected exactly 1 http_route :EntryPoint; got {}: {:?}",
        nodes.len(),
        nodes.iter().map(|n| &n.id).collect::<Vec<_>>(),
    );
    let node = &nodes[0];
    let name = node
        .props
        .get("name")
        .and_then(PropValue::as_str)
        .expect("name prop");
    assert_eq!(name, expected_path, "path literal mismatch");
    let handler = node
        .props
        .get("handler_qname")
        .and_then(PropValue::as_str)
        .expect("handler_qname prop");
    assert_eq!(handler, expected_handler_qname, "handler qname mismatch");

    let expected_target = item_node_id(expected_handler_qname);
    let matching = edges
        .iter()
        .filter(|e| e.dst == expected_target && e.src == node.id)
        .count();
    assert_eq!(
        matching,
        1,
        "expected exactly 1 EXPOSES edge {} -> {}; edges: {:?}",
        node.id,
        expected_target,
        edges.iter().map(|e| (&e.src, &e.dst)).collect::<Vec<_>>(),
    );
}

#[test]
fn axum_route_method_emits_http_route_entry_point() {
    let tmp = tempdir().expect("tempdir");
    write_workspace_with_dep(
        tmp.path(),
        r#"
// Stand-in for axum::Router — the scan is method-name-syntactic.
pub struct Router;
impl Router {
    pub fn new() -> Self { Router }
    pub fn route<H>(self, _path: &str, _handler: H) -> Self { self }
}

pub fn list_users() {}

pub fn build() -> Router {
    Router::new().route("/users", list_users)
}
"#,
        "axum",
    );
    let (nodes, edges) = http_routes(tmp.path());
    assert_one_route(&nodes, &edges, "/users", "routes::list_users");
}

#[test]
fn axum_get_method_emits_http_route_entry_point() {
    let tmp = tempdir().expect("tempdir");
    write_workspace_with_dep(
        tmp.path(),
        r#"
pub struct Router;
impl Router {
    pub fn new() -> Self { Router }
    pub fn get<H>(self, _path: &str, _handler: H) -> Self { self }
}

pub fn show_user() {}

pub fn build() -> Router {
    Router::new().get("/users/:id", show_user)
}
"#,
        "axum",
    );
    let (nodes, edges) = http_routes(tmp.path());
    assert_one_route(&nodes, &edges, "/users/:id", "routes::show_user");
}

#[test]
fn axum_post_method_emits_http_route_entry_point() {
    let tmp = tempdir().expect("tempdir");
    write_workspace_with_dep(
        tmp.path(),
        r#"
pub struct Router;
impl Router {
    pub fn new() -> Self { Router }
    pub fn post<H>(self, _path: &str, _handler: H) -> Self { self }
}

pub fn create_user() {}

pub fn build() -> Router {
    Router::new().post("/users", create_user)
}
"#,
        "axum",
    );
    let (nodes, edges) = http_routes(tmp.path());
    assert_one_route(&nodes, &edges, "/users", "routes::create_user");
}

#[test]
fn axum_nest_method_emits_http_route_entry_point() {
    let tmp = tempdir().expect("tempdir");
    write_workspace_with_dep(
        tmp.path(),
        r#"
pub struct Router;
impl Router {
    pub fn new() -> Self { Router }
    pub fn nest(self, _prefix: &str, _router: Router) -> Self { self }
}

pub fn api_router() -> Router { Router::new() }

pub fn build() -> Router {
    Router::new().nest("/api", api_router())
}
"#,
        "axum",
    );
    let (nodes, edges) = http_routes(tmp.path());
    assert_one_route(&nodes, &edges, "/api", "routes::api_router");
}

#[test]
fn actix_app_route_with_web_get_to_emits_http_route_entry_point() {
    let tmp = tempdir().expect("tempdir");
    write_workspace_with_dep(
        tmp.path(),
        r#"
// Stand-ins for actix_web::App / actix_web::web.
pub struct App;
impl App {
    pub fn new() -> Self { App }
    pub fn route(self, _path: &str, _route: Route) -> Self { self }
}
pub struct Route;
impl Route {
    pub fn to<H>(self, _handler: H) -> Route { self }
}
pub mod web {
    pub fn get() -> super::Route { super::Route }
}

pub fn index() {}

pub fn build() -> App {
    App::new().route("/", web::get().to(index))
}
"#,
        "actix_web",
    );
    let (nodes, edges) = http_routes(tmp.path());
    assert_one_route(&nodes, &edges, "/", "routes::index");
}

#[test]
fn map_put_with_non_slash_key_is_not_a_route() {
    let tmp = tempdir().expect("tempdir");
    write_workspace_with_dep(
        tmp.path(),
        r#"
pub struct Port;
impl Port {
    pub fn put(&self, _key: &str, _value: Quote) {}
    pub fn get(&self, _key: &str) -> Option<Quote> { None }
    pub fn delete(&self, _key: &str) {}
    pub fn post(&self, _key: &str, _value: Quote) {}
}
pub struct Quote;
pub fn quote(_ticker: &str, _price: i64) -> Quote { Quote }

pub fn exercise() {
    let p = Port;
    p.put("BTC/USD", quote("BTC/USD", 65000));
    p.get("ETH/USD");
    p.delete("SOL/USD");
    p.post("EUR/USD", quote("EUR/USD", 1));
}
"#,
        "axum",
    );
    let (nodes, _edges) = http_routes(tmp.path());
    assert!(
        nodes.is_empty(),
        "expected zero http_route :EntryPoints on a map-style `.put/get/delete/post` fixture \
         where literal keys do not start with `/`; got {} nodes: {:?}",
        nodes.len(),
        nodes
            .iter()
            .map(|n| n.props.get("name").and_then(PropValue::as_str))
            .collect::<Vec<_>>(),
    );
}

#[test]
fn actix_service_web_resource_route_emits_http_route_entry_point() {
    let tmp = tempdir().expect("tempdir");
    write_workspace_with_dep(
        tmp.path(),
        r#"
pub struct App;
impl App {
    pub fn new() -> Self { App }
    pub fn service(self, _svc: Resource) -> Self { self }
}
pub struct Resource;
impl Resource {
    pub fn route(self, _route: Route) -> Resource { self }
    pub fn to<H>(self, _handler: H) -> Resource { self }
}
pub struct Route;
impl Route {
    pub fn to<H>(self, _handler: H) -> Route { self }
}
pub mod web {
    pub fn resource(_path: &str) -> super::Resource { super::Resource }
    pub fn get() -> super::Route { super::Route }
}

pub fn health() {}

pub fn build() -> App {
    App::new().service(web::resource("/health").route(web::get().to(health)))
}
"#,
        "actix_web",
    );
    let (nodes, edges) = http_routes(tmp.path());
    assert_one_route(&nodes, &edges, "/health", "routes::health");
}

#[test]
fn http_route_detector_inert_without_framework_dependency() {
    let tmp = tempdir().expect("tempdir");
    write_workspace(
        tmp.path(),
        r#"
pub struct Router;
impl Router {
    pub fn new() -> Self { Router }
    pub fn route<H>(self, _path: &str, _handler: H) -> Self { self }
}

pub fn list_users() {}

pub fn build() -> Router {
    Router::new().route("/users", list_users)
}
"#,
    );
    let (nodes, _edges) = http_routes(tmp.path());
    assert!(
        nodes.is_empty(),
        "http_route detector must be inert without an axum/actix dependency; got {} \
         http_route :EntryPoint(s): {:?}",
        nodes.len(),
        nodes.iter().map(|n| &n.id).collect::<Vec<_>>(),
    );
}
