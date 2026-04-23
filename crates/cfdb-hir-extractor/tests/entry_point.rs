//! Integration test for `extract_entry_points` (slice 4, Issue #86 +
//! Issue #125 extension).
//!
//! Validates attribute-based heuristic detection of clap CLI commands
//! (via `#[derive(Parser)]` / `#[derive(Subcommand)]`) and MCP tools
//! (via `#[tool]`); plus call-expression-level detection of cron jobs
//! (`tokio_cron_scheduler::Job::new_async` / `Job::new` /
//! `JobScheduler::add`) and websocket upgrade handlers
//! (`WebSocketUpgrade::on_upgrade`) — Issue #125.
//!
//! The v0.2 vocabulary covers five `:EntryPoint` kinds: `cli_command`,
//! `mcp_tool` (Issue #86), `cron_job`, `websocket` (this file) and the
//! sibling `http_route` slice (#124). Each kind gets its own fixture
//! file with one or more call shapes to exercise so a regression in
//! any shape fails its own assertion rather than hiding behind a
//! cross-kind aggregate.

use std::fs;
use std::path::Path;

use cfdb_core::fact::{Node, PropValue};
use cfdb_core::qname::{field_node_id, item_node_id, variant_node_id};
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

fn workspace_cargo_toml(members: &[&str]) -> String {
    let quoted: Vec<String> = members.iter().map(|m| format!("    \"{m}\"")).collect();
    format!(
        "[workspace]\nresolver = \"2\"\nmembers = [\n{}\n]\n",
        quoted.join(",\n")
    )
}

fn member_cargo_toml(name: &str) -> String {
    format!(
        "[package]\nname = \"{name}\"\nversion = \"0.0.1\"\nedition = \"2021\"\n\n[dependencies]\n"
    )
}

fn entry_points(nodes: &[Node]) -> Vec<&Node> {
    nodes
        .iter()
        .filter(|n| n.label.as_str() == Label::ENTRY_POINT)
        .collect()
}

fn kind_of(n: &Node) -> Option<&str> {
    n.props.get("kind").and_then(PropValue::as_str)
}

fn handler_qname(n: &Node) -> Option<&str> {
    n.props.get("handler_qname").and_then(PropValue::as_str)
}

fn cron_expr_of(n: &Node) -> Option<&str> {
    n.props.get("cron_expr").and_then(PropValue::as_str)
}

#[test]
fn attribute_based_entry_point_detection_covers_cli_and_mcp() {
    let tmp = tempdir().expect("tempdir");
    let root = tmp.path();

    write(root, "Cargo.toml", &workspace_cargo_toml(&["epfixture"]));
    // We do NOT pull in actual `clap` or `rmcp` crates — the fixture
    // only needs the attributes textually; the HIR extractor's scan
    // is attribute-syntactic, not trait-resolution-based.
    write(
        root,
        "epfixture/Cargo.toml",
        &member_cargo_toml("epfixture"),
    );
    write(
        root,
        "epfixture/src/lib.rs",
        r#"
// Stand-ins for clap derives — the scan is textual, so a bare
// Parser/Subcommand identifier is sufficient. Real consumers use
// clap::Parser; the heuristic matches both.
pub trait Parser {}
pub trait Subcommand {}

#[derive(Parser)]
pub struct Cli {
    pub arg: String,
}

#[derive(Subcommand)]
pub enum Command {
    Run,
    Stop,
}

// Stand-in for an MCP-style tool attribute. The heuristic matches
// the last path segment `tool` regardless of the crate.
#[tool]
pub fn echo(input: &str) -> String {
    input.to_string()
}

pub fn unrelated_fn() {}
"#,
    );

    let (db, vfs) = build_hir_database(root).expect("build_hir_database on epfixture");
    let (nodes, edges) =
        extract_entry_points(&db, &vfs).expect("extract_entry_points on epfixture");

    // Filter :EntryPoint nodes.
    let eps = entry_points(&nodes);

    // Expect exactly 3: Cli (cli_command), Command (cli_command), echo (mcp_tool).
    assert_eq!(
        eps.len(),
        3,
        "expected 3 :EntryPoint nodes (Cli, Command, echo); got {}: {:?}",
        eps.len(),
        eps.iter().map(|n| &n.id).collect::<Vec<_>>(),
    );

    let cli_count = eps
        .iter()
        .filter(|n| kind_of(n) == Some("cli_command"))
        .count();
    let mcp_count = eps
        .iter()
        .filter(|n| kind_of(n) == Some("mcp_tool"))
        .count();
    assert_eq!(cli_count, 2, "expected 2 cli_command :EntryPoint");
    assert_eq!(mcp_count, 1, "expected 1 mcp_tool :EntryPoint");

    // Each :EntryPoint must have an EXPOSES edge to the handler Item.
    let exposes: Vec<_> = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::EXPOSES)
        .collect();
    assert_eq!(
        exposes.len(),
        3,
        "expected 3 EXPOSES edges (one per :EntryPoint); got {}",
        exposes.len()
    );

    // Spot-check: the `echo` mcp_tool's EXPOSES edge points to
    // item:epfixture::echo.
    let expected_handler = item_node_id("epfixture::echo");
    assert!(
        exposes.iter().any(|e| e.dst == expected_handler),
        "expected EXPOSES edge → {}; saw: {:?}",
        expected_handler,
        exposes.iter().map(|e| &e.dst).collect::<Vec<_>>(),
    );

    // unrelated_fn must NOT appear anywhere.
    assert!(
        !eps.iter()
            .any(|n| handler_qname(n).is_some_and(|q| q.ends_with("unrelated_fn"))),
        "unrelated_fn must not be detected as an entry point",
    );
}

// ---------------------------------------------------------------
// Issue #125 — cron_job (tokio_cron_scheduler)
// ---------------------------------------------------------------

#[test]
fn cron_job_detects_job_new_async_with_named_registration_fn() {
    // `Job::new_async("<cron>", |_, _| async { ... })` inside
    // `register_jobs` — the :EntryPoint EXPOSES the enclosing fn
    // (the closure itself has no path-level qname).
    let tmp = tempdir().expect("tempdir");
    let root = tmp.path();
    write(root, "Cargo.toml", &workspace_cargo_toml(&["cronfix"]));
    write(root, "cronfix/Cargo.toml", &member_cargo_toml("cronfix"));
    write(
        root,
        "cronfix/src/lib.rs",
        r#"
// Stand-ins for tokio_cron_scheduler types. Heuristic is textual on
// the call chain `Job::new_async(<cron-literal>, <closure>)`.
pub struct Job;
impl Job {
    pub fn new_async<F>(_cron: &str, _f: F) -> Self { Job }
}

pub fn register_jobs() {
    let _j = Job::new_async("0 * * * * *", |_, _| async {});
}
"#,
    );

    let (db, vfs) = build_hir_database(root).expect("build_hir_database on cronfix");
    let (nodes, edges) = extract_entry_points(&db, &vfs).expect("extract_entry_points on cronfix");

    let eps: Vec<_> = entry_points(&nodes)
        .into_iter()
        .filter(|n| kind_of(n) == Some("cron_job"))
        .collect();
    assert_eq!(
        eps.len(),
        1,
        "expected exactly 1 cron_job :EntryPoint; got {}: {:?}",
        eps.len(),
        eps.iter().map(|n| &n.id).collect::<Vec<_>>(),
    );

    let ep = eps[0];
    assert_eq!(
        cron_expr_of(ep),
        Some("0 * * * * *"),
        "cron_expr prop must carry the literal schedule string"
    );
    assert_eq!(
        handler_qname(ep),
        Some("cronfix::register_jobs"),
        "cron_job handler_qname must be the enclosing fn (closure body has no qname)"
    );

    // EXPOSES edge → item:cronfix::register_jobs.
    let expected = item_node_id("cronfix::register_jobs");
    assert!(
        edges
            .iter()
            .any(|e| e.label.as_str() == EdgeLabel::EXPOSES && e.dst == expected),
        "expected EXPOSES edge to {expected}"
    );
}

#[test]
fn cron_job_detects_job_new_synchronous_variant() {
    // `Job::new("<cron>", |_, _| { ... })` — the sync sibling of
    // new_async. Same dispatch arm.
    let tmp = tempdir().expect("tempdir");
    let root = tmp.path();
    write(root, "Cargo.toml", &workspace_cargo_toml(&["cronsync"]));
    write(root, "cronsync/Cargo.toml", &member_cargo_toml("cronsync"));
    write(
        root,
        "cronsync/src/lib.rs",
        r#"
pub struct Job;
impl Job {
    pub fn new<F>(_cron: &str, _f: F) -> Self { Job }
}

pub fn install_daily() {
    let _j = Job::new("@daily", |_, _| {});
}
"#,
    );

    let (db, vfs) = build_hir_database(root).expect("build_hir_database on cronsync");
    let (nodes, _edges) =
        extract_entry_points(&db, &vfs).expect("extract_entry_points on cronsync");

    let eps: Vec<_> = entry_points(&nodes)
        .into_iter()
        .filter(|n| kind_of(n) == Some("cron_job"))
        .collect();
    assert_eq!(eps.len(), 1, "expected 1 cron_job via Job::new");
    assert_eq!(cron_expr_of(eps[0]), Some("@daily"));
    assert_eq!(handler_qname(eps[0]), Some("cronsync::install_daily"));
}

#[test]
fn cron_job_detects_scheduler_add_registration_path() {
    // `JobScheduler::add(Job::new_async(...))` — the registration
    // wrapper call. The inner Job::new_async still fires the emitter.
    let tmp = tempdir().expect("tempdir");
    let root = tmp.path();
    write(root, "Cargo.toml", &workspace_cargo_toml(&["cronsched"]));
    write(
        root,
        "cronsched/Cargo.toml",
        &member_cargo_toml("cronsched"),
    );
    write(
        root,
        "cronsched/src/lib.rs",
        r#"
pub struct Job;
impl Job {
    pub fn new_async<F>(_cron: &str, _f: F) -> Self { Job }
}
pub struct JobScheduler;
impl JobScheduler {
    pub fn add(_j: Job) {}
}

pub fn boot() {
    JobScheduler::add(Job::new_async("*/5 * * * * *", |_, _| async {}));
}
"#,
    );

    let (db, vfs) = build_hir_database(root).expect("build_hir_database on cronsched");
    let (nodes, edges) =
        extract_entry_points(&db, &vfs).expect("extract_entry_points on cronsched");

    let eps: Vec<_> = entry_points(&nodes)
        .into_iter()
        .filter(|n| kind_of(n) == Some("cron_job"))
        .collect();
    assert_eq!(
        eps.len(),
        1,
        "expected 1 cron_job when wrapped in JobScheduler::add"
    );
    assert_eq!(cron_expr_of(eps[0]), Some("*/5 * * * * *"));
    assert_eq!(handler_qname(eps[0]), Some("cronsched::boot"));

    let expected = item_node_id("cronsched::boot");
    assert!(
        edges
            .iter()
            .any(|e| e.label.as_str() == EdgeLabel::EXPOSES && e.dst == expected),
        "expected EXPOSES edge to {expected}"
    );
}

// ---------------------------------------------------------------
// Issue #125 — websocket (axum WebSocketUpgrade::on_upgrade)
// ---------------------------------------------------------------

#[test]
fn websocket_detects_on_upgrade_with_named_handler_fn() {
    // `ws.on_upgrade(ws_handler)` — the callee arg is a path to a
    // named fn; EXPOSES target is that fn's qname.
    let tmp = tempdir().expect("tempdir");
    let root = tmp.path();
    write(root, "Cargo.toml", &workspace_cargo_toml(&["wsnamed"]));
    write(root, "wsnamed/Cargo.toml", &member_cargo_toml("wsnamed"));
    write(
        root,
        "wsnamed/src/lib.rs",
        r#"
// Stand-in for axum::extract::ws::{WebSocketUpgrade, WebSocket}.
pub struct WebSocket;
pub struct WebSocketUpgrade;
impl WebSocketUpgrade {
    pub fn on_upgrade<F>(self, _f: F) -> Response where F: FnOnce(WebSocket) {
        Response
    }
}
pub struct Response;

pub fn ws_handler(_socket: WebSocket) {}

pub fn mount_ws(upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(ws_handler)
}
"#,
    );

    let (db, vfs) = build_hir_database(root).expect("build_hir_database on wsnamed");
    let (nodes, edges) = extract_entry_points(&db, &vfs).expect("extract_entry_points on wsnamed");

    let eps: Vec<_> = entry_points(&nodes)
        .into_iter()
        .filter(|n| kind_of(n) == Some("websocket"))
        .collect();
    assert_eq!(
        eps.len(),
        1,
        "expected 1 websocket :EntryPoint from on_upgrade(named_fn)"
    );
    assert_eq!(
        handler_qname(eps[0]),
        Some("wsnamed::ws_handler"),
        "named-fn handler resolves to path-argument qname"
    );

    let expected = item_node_id("wsnamed::ws_handler");
    assert!(
        edges
            .iter()
            .any(|e| e.label.as_str() == EdgeLabel::EXPOSES && e.dst == expected),
        "expected EXPOSES edge to {expected}"
    );
}

#[test]
fn websocket_detects_on_upgrade_with_inline_closure() {
    // `ws.on_upgrade(|socket| async { ... })` — closure has no
    // path; EXPOSES target is the enclosing fn (same policy as
    // cron_job's closure bodies).
    let tmp = tempdir().expect("tempdir");
    let root = tmp.path();
    write(root, "Cargo.toml", &workspace_cargo_toml(&["wsclosure"]));
    write(
        root,
        "wsclosure/Cargo.toml",
        &member_cargo_toml("wsclosure"),
    );
    write(
        root,
        "wsclosure/src/lib.rs",
        r#"
pub struct WebSocket;
pub struct WebSocketUpgrade;
impl WebSocketUpgrade {
    pub fn on_upgrade<F>(self, _f: F) -> Response where F: FnOnce(WebSocket) {
        Response
    }
}
pub struct Response;

pub fn mount_ws_inline(upgrade: WebSocketUpgrade) -> Response {
    upgrade.on_upgrade(|_socket| {})
}
"#,
    );

    let (db, vfs) = build_hir_database(root).expect("build_hir_database on wsclosure");
    let (nodes, edges) =
        extract_entry_points(&db, &vfs).expect("extract_entry_points on wsclosure");

    let eps: Vec<_> = entry_points(&nodes)
        .into_iter()
        .filter(|n| kind_of(n) == Some("websocket"))
        .collect();
    assert_eq!(eps.len(), 1, "expected 1 websocket :EntryPoint for closure");
    assert_eq!(
        handler_qname(eps[0]),
        Some("wsclosure::mount_ws_inline"),
        "closure handler falls back to enclosing fn qname"
    );

    let expected = item_node_id("wsclosure::mount_ws_inline");
    assert!(
        edges
            .iter()
            .any(|e| e.label.as_str() == EdgeLabel::EXPOSES && e.dst == expected),
        "expected EXPOSES edge to {expected}"
    );
}

// ---------------------------------------------------------------
// Issue #219 — REGISTERS_PARAM producer (clap + Subcommand paths)
// ---------------------------------------------------------------
//
// The HIR-side producer owns two rows of the §3.1 crate-ownership
// table:
//
// - `#[derive(Parser)]` struct → one REGISTERS_PARAM edge per
//   `#[arg(...)]`-carrying named field, pointing at the syn-side
//   `:Field` node id produced via `field_node_id`.
// - `#[derive(Subcommand)]` enum → one REGISTERS_PARAM edge per
//   declared variant (the transitional approximation from §3.1 N1),
//   pointing at the syn-side `:Variant` node id produced via
//   `variant_node_id`.
//
// The HIR side does NOT emit `:Field` / `:Variant` nodes — only edges.
// In a full `cfdb extract --features hir` run the syn-side pipeline
// emits the target nodes; these tests only assert the edge shape
// because the HIR harness here runs `extract_entry_points` in
// isolation.

#[test]
fn clap_parser_struct_emits_one_registers_param_per_arg_field() {
    // `#[derive(Parser)]` struct with 3 `#[arg]` fields + 1 plain
    // field (no `#[arg]`). Expect exactly 3 REGISTERS_PARAM edges,
    // dsts = field_node_id(struct_qname, field_name) for each.
    let tmp = tempdir().expect("tempdir");
    let root = tmp.path();
    write(root, "Cargo.toml", &workspace_cargo_toml(&["clapargs"]));
    write(root, "clapargs/Cargo.toml", &member_cargo_toml("clapargs"));
    write(
        root,
        "clapargs/src/lib.rs",
        r#"
// Stand-in for clap's Parser derive — the producer detects the
// derive syntactically (via `has_clap_derive`). The `#[arg(...)]`
// helper attribute is also matched syntactically (last path segment
// `arg`); ra_ap_syntax parses these helper attrs as plain
// attributes regardless of whether `Parser` actually declares `arg`
// as a helper in a real macro definition.
pub trait Parser {}

#[derive(Parser)]
pub struct Cli {
    #[arg(short, long)]
    pub input: String,
    #[arg(long)]
    pub count: u32,
    #[arg]
    pub verbose: bool,
    pub internal_only: String,
}
"#,
    );

    let (db, vfs) = build_hir_database(root).expect("build_hir_database on clapargs");
    let (_nodes, edges) =
        extract_entry_points(&db, &vfs).expect("extract_entry_points on clapargs");

    let struct_qname = "clapargs::Cli";
    let entry_point_id = format!("entrypoint:cli_command:{struct_qname}");
    let register_edges: Vec<_> = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::REGISTERS_PARAM && e.src == entry_point_id)
        .collect();
    assert_eq!(
        register_edges.len(),
        3,
        "expected 3 REGISTERS_PARAM edges for 3 #[arg] fields; got {}: {:?}",
        register_edges.len(),
        register_edges
            .iter()
            .map(|e| (&e.src, &e.dst))
            .collect::<Vec<_>>(),
    );

    let mut dsts: Vec<&str> = register_edges.iter().map(|e| e.dst.as_str()).collect();
    dsts.sort();
    let expected = [
        field_node_id(struct_qname, "count"),
        field_node_id(struct_qname, "input"),
        field_node_id(struct_qname, "verbose"),
    ];
    let expected_refs: Vec<&str> = expected.iter().map(String::as_str).collect();
    assert_eq!(
        dsts, expected_refs,
        "REGISTERS_PARAM dsts must equal field_node_id(struct_qname, <arg-field-name>)"
    );
}

#[test]
fn clap_subcommand_enum_emits_one_registers_param_per_variant() {
    // `#[derive(Subcommand)]` enum with 3 variants — expect 3
    // REGISTERS_PARAM edges, dsts = variant_node_id(enum_qname, i)
    // for i ∈ [0, 1, 2] (declaration order). This is the transitional
    // approximation from §3.1 N1; per-variant-field granularity is a
    // follow-up RFC.
    let tmp = tempdir().expect("tempdir");
    let root = tmp.path();
    write(root, "Cargo.toml", &workspace_cargo_toml(&["subcmd"]));
    write(root, "subcmd/Cargo.toml", &member_cargo_toml("subcmd"));
    write(
        root,
        "subcmd/src/lib.rs",
        r#"
pub trait Subcommand {}

#[derive(Subcommand)]
pub enum Command {
    Run,
    Stop { force: bool },
    Status(String),
}
"#,
    );

    let (db, vfs) = build_hir_database(root).expect("build_hir_database on subcmd");
    let (_nodes, edges) = extract_entry_points(&db, &vfs).expect("extract_entry_points on subcmd");

    let enum_qname = "subcmd::Command";
    let entry_point_id = format!("entrypoint:cli_command:{enum_qname}");
    let register_edges: Vec<_> = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::REGISTERS_PARAM && e.src == entry_point_id)
        .collect();
    assert_eq!(
        register_edges.len(),
        3,
        "expected 3 REGISTERS_PARAM edges for 3 variants; got {}: {:?}",
        register_edges.len(),
        register_edges
            .iter()
            .map(|e| (&e.src, &e.dst))
            .collect::<Vec<_>>(),
    );

    let mut dsts: Vec<&str> = register_edges.iter().map(|e| e.dst.as_str()).collect();
    dsts.sort();
    let expected = [
        variant_node_id(enum_qname, 0),
        variant_node_id(enum_qname, 1),
        variant_node_id(enum_qname, 2),
    ];
    let mut expected_refs: Vec<&str> = expected.iter().map(String::as_str).collect();
    expected_refs.sort();
    assert_eq!(
        dsts, expected_refs,
        "REGISTERS_PARAM dsts must equal variant_node_id(enum_qname, i) for i ∈ [0, 1, 2]"
    );
}

#[test]
fn clap_parser_struct_with_no_arg_fields_emits_zero_registers_param() {
    // `#[derive(Parser)]` struct with zero `#[arg]`-annotated fields —
    // the :EntryPoint still emits (the struct itself is recognised),
    // but REGISTERS_PARAM count is zero.
    let tmp = tempdir().expect("tempdir");
    let root = tmp.path();
    write(root, "Cargo.toml", &workspace_cargo_toml(&["noargs"]));
    write(root, "noargs/Cargo.toml", &member_cargo_toml("noargs"));
    write(
        root,
        "noargs/src/lib.rs",
        r#"
pub trait Parser {}

#[derive(Parser)]
pub struct Cli {
    pub plain: String,
}
"#,
    );

    let (db, vfs) = build_hir_database(root).expect("build_hir_database on noargs");
    let (nodes, edges) = extract_entry_points(&db, &vfs).expect("extract_entry_points on noargs");

    // Sanity: the :EntryPoint still emits.
    let eps = entry_points(&nodes);
    assert_eq!(eps.len(), 1, "Parser struct still emits :EntryPoint");

    // But no REGISTERS_PARAM edges.
    let register_edges: Vec<_> = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::REGISTERS_PARAM)
        .collect();
    assert!(
        register_edges.is_empty(),
        "zero #[arg] fields → zero REGISTERS_PARAM edges; got {:?}",
        register_edges
            .iter()
            .map(|e| (&e.src, &e.dst))
            .collect::<Vec<_>>(),
    );
}
