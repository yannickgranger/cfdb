use std::fs;
use std::path::Path;

use cfdb_core::fact::{Node, PropValue};
use cfdb_core::qname::{field_node_id, item_node_id, method_qname, param_node_id, variant_node_id};
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

fn member_cargo_toml_with_deps(name: &str, deps: &[&str]) -> String {
    let mut manifest = member_cargo_toml(name);
    for dep in deps {
        manifest.push_str(&format!("{dep} = {{ path = \"../{dep}\" }}\n"));
    }
    manifest
}

fn write_stub_crate(root: &Path, name: &str) {
    write(
        root,
        &format!("{name}/Cargo.toml"),
        &member_cargo_toml(name),
    );
    write(root, &format!("{name}/src/lib.rs"), "");
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

    write(
        root,
        "Cargo.toml",
        &workspace_cargo_toml(&["epfixture", "clap"]),
    );
    write(
        root,
        "epfixture/Cargo.toml",
        &member_cargo_toml_with_deps("epfixture", &["clap"]),
    );
    write_stub_crate(root, "clap");
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

    let (db, vfs, _pm_client, targets) =
        build_hir_database(root, false).expect("build_hir_database on epfixture");
    let (nodes, edges) =
        extract_entry_points(&db, &vfs, root, &targets).expect("extract_entry_points on epfixture");

    let eps = entry_points(&nodes);

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

    let expected_handler = item_node_id("epfixture::echo");
    assert!(
        exposes.iter().any(|e| e.dst == expected_handler),
        "expected EXPOSES edge → {}; saw: {:?}",
        expected_handler,
        exposes.iter().map(|e| &e.dst).collect::<Vec<_>>(),
    );

    assert!(
        !eps.iter()
            .any(|n| handler_qname(n).is_some_and(|q| q.ends_with("unrelated_fn"))),
        "unrelated_fn must not be detected as an entry point",
    );
}

#[test]
fn cron_job_detects_job_new_async_with_named_registration_fn() {
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

    let (db, vfs, _pm_client, targets) =
        build_hir_database(root, false).expect("build_hir_database on cronfix");
    let (nodes, edges) =
        extract_entry_points(&db, &vfs, root, &targets).expect("extract_entry_points on cronfix");

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

    let (db, vfs, _pm_client, targets) =
        build_hir_database(root, false).expect("build_hir_database on cronsync");
    let (nodes, _edges) =
        extract_entry_points(&db, &vfs, root, &targets).expect("extract_entry_points on cronsync");

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

    let (db, vfs, _pm_client, targets) =
        build_hir_database(root, false).expect("build_hir_database on cronsched");
    let (nodes, edges) =
        extract_entry_points(&db, &vfs, root, &targets).expect("extract_entry_points on cronsched");

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

#[test]
fn websocket_detects_on_upgrade_with_named_handler_fn() {
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

    let (db, vfs, _pm_client, targets) =
        build_hir_database(root, false).expect("build_hir_database on wsnamed");
    let (nodes, edges) =
        extract_entry_points(&db, &vfs, root, &targets).expect("extract_entry_points on wsnamed");

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

    let (db, vfs, _pm_client, targets) =
        build_hir_database(root, false).expect("build_hir_database on wsclosure");
    let (nodes, edges) =
        extract_entry_points(&db, &vfs, root, &targets).expect("extract_entry_points on wsclosure");

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

#[test]
fn clap_parser_struct_emits_one_registers_param_per_arg_field() {
    let tmp = tempdir().expect("tempdir");
    let root = tmp.path();
    write(
        root,
        "Cargo.toml",
        &workspace_cargo_toml(&["clapargs", "clap"]),
    );
    write(
        root,
        "clapargs/Cargo.toml",
        &member_cargo_toml_with_deps("clapargs", &["clap"]),
    );
    write_stub_crate(root, "clap");
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

    let (db, vfs, _pm_client, targets) =
        build_hir_database(root, false).expect("build_hir_database on clapargs");
    let (_nodes, edges) =
        extract_entry_points(&db, &vfs, root, &targets).expect("extract_entry_points on clapargs");

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
    let tmp = tempdir().expect("tempdir");
    let root = tmp.path();
    write(
        root,
        "Cargo.toml",
        &workspace_cargo_toml(&["subcmd", "clap"]),
    );
    write(
        root,
        "subcmd/Cargo.toml",
        &member_cargo_toml_with_deps("subcmd", &["clap"]),
    );
    write_stub_crate(root, "clap");
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

    let (db, vfs, _pm_client, targets) =
        build_hir_database(root, false).expect("build_hir_database on subcmd");
    let (_nodes, edges) =
        extract_entry_points(&db, &vfs, root, &targets).expect("extract_entry_points on subcmd");

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
    let tmp = tempdir().expect("tempdir");
    let root = tmp.path();
    write(
        root,
        "Cargo.toml",
        &workspace_cargo_toml(&["noargs", "clap"]),
    );
    write(
        root,
        "noargs/Cargo.toml",
        &member_cargo_toml_with_deps("noargs", &["clap"]),
    );
    write_stub_crate(root, "clap");
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

    let (db, vfs, _pm_client, targets) =
        build_hir_database(root, false).expect("build_hir_database on noargs");
    let (nodes, edges) =
        extract_entry_points(&db, &vfs, root, &targets).expect("extract_entry_points on noargs");

    let eps = entry_points(&nodes);
    assert_eq!(eps.len(), 1, "Parser struct still emits :EntryPoint");

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

#[test]
fn clap_detector_inert_without_clap_dependency() {
    let tmp = tempdir().expect("tempdir");
    let root = tmp.path();
    write(root, "Cargo.toml", &workspace_cargo_toml(&["nodep"]));
    write(root, "nodep/Cargo.toml", &member_cargo_toml("nodep"));
    write(
        root,
        "nodep/src/lib.rs",
        r#"
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
"#,
    );

    let (db, vfs, _pm_client, targets) =
        build_hir_database(root, false).expect("build_hir_database on nodep");
    let (nodes, _edges) =
        extract_entry_points(&db, &vfs, root, &targets).expect("extract_entry_points on nodep");

    let cli: Vec<_> = entry_points(&nodes)
        .into_iter()
        .filter(|n| kind_of(n) == Some("cli_command"))
        .collect();
    assert!(
        cli.is_empty(),
        "clap detector must be inert without a `clap` dependency; got {} cli_command \
         :EntryPoint(s): {:?}",
        cli.len(),
        cli.iter().map(|n| &n.id).collect::<Vec<_>>(),
    );
}

#[test]
fn clap_detector_inert_when_clap_is_transitive_only() {
    let tmp = tempdir().expect("tempdir");
    let base = tmp.path();

    write(base, "ws/Cargo.toml", &workspace_cargo_toml(&["consumer"]));
    write(
        base,
        "ws/consumer/Cargo.toml",
        "[package]\nname = \"consumer\"\nversion = \"0.0.1\"\nedition = \"2021\"\n\n\
         [dependencies]\nmidlib = { path = \"../../midlib\" }\n",
    );
    write(
        base,
        "ws/consumer/src/lib.rs",
        r#"
pub trait Parser {}

#[derive(Parser)]
pub struct Cli {
    pub arg: String,
}
"#,
    );
    write(
        base,
        "midlib/Cargo.toml",
        "[package]\nname = \"midlib\"\nversion = \"0.0.1\"\nedition = \"2021\"\n\n\
         [dependencies]\nclap = { path = \"../clap\" }\n",
    );
    write(base, "midlib/src/lib.rs", "");
    write(base, "clap/Cargo.toml", &member_cargo_toml("clap"));
    write(base, "clap/src/lib.rs", "");

    let ws_root = base.join("ws");
    let (db, vfs, _pm_client, targets) =
        build_hir_database(&ws_root, false).expect("build_hir_database on transitive-clap");
    let (nodes, _edges) = extract_entry_points(&db, &vfs, &ws_root, &targets)
        .expect("extract_entry_points on transitive-clap");

    let cli: Vec<_> = entry_points(&nodes)
        .into_iter()
        .filter(|n| kind_of(n) == Some("cli_command"))
        .collect();
    assert!(
        cli.is_empty(),
        "clap detector must be inert when `clap` is only a transitive (non-member) dependency; \
         got {} cli_command :EntryPoint(s): {:?}",
        cli.len(),
        cli.iter().map(|n| &n.id).collect::<Vec<_>>(),
    );
}

#[test]
fn mcp_tool_on_impl_method_emits_registers_param_matching_syn_side_param_id() {
    let tmp = tempdir().expect("tempdir");
    let root = tmp.path();
    write(root, "Cargo.toml", &workspace_cargo_toml(&["impltools"]));
    write(
        root,
        "impltools/Cargo.toml",
        &member_cargo_toml("impltools"),
    );
    write(
        root,
        "impltools/src/lib.rs",
        r#"
// Stand-in receiver — the test exercises the impl-method qname path.
pub struct Tools;

impl Tools {
    // `#[tool]` attribute detected syntactically by `has_tool_attr`.
    // `&self` receiver + two typed params; the syn-side extractor
    // emits :Param at index 0 (self), 1 (x), 2 (y); the HIR-side
    // REGISTERS_PARAM emitter offsets typed params by +1 when a
    // receiver is present, so it targets indices 1 and 2.
    #[tool]
    pub fn bar(&self, x: i32, y: i32) -> i32 {
        x + y
    }
}
"#,
    );

    let (db, vfs, _pm_client, targets) =
        build_hir_database(root, false).expect("build_hir_database on impltools");
    let (nodes, edges) =
        extract_entry_points(&db, &vfs, root, &targets).expect("extract_entry_points on impltools");

    let expected_qname = method_qname(&["impltools".to_string()], "Tools", "bar");
    assert_eq!(
        expected_qname, "impltools::Tools::bar",
        "sanity: method_qname formula must yield `<crate>::<target>::<method>`"
    );

    let eps = entry_points(&nodes);
    let mcp_eps: Vec<_> = eps
        .iter()
        .filter(|n| kind_of(n) == Some("mcp_tool"))
        .collect();
    assert_eq!(
        mcp_eps.len(),
        1,
        "expected exactly 1 mcp_tool :EntryPoint for impl method; got {}: {:?}",
        mcp_eps.len(),
        mcp_eps.iter().map(|n| &n.id).collect::<Vec<_>>(),
    );
    let ep = mcp_eps[0];
    assert_eq!(
        handler_qname(ep),
        Some(expected_qname.as_str()),
        "handler_qname must include impl target: expected `{expected_qname}`, got `{:?}`",
        handler_qname(ep),
    );

    let expected_item = item_node_id(&expected_qname);
    let exposes: Vec<_> = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::EXPOSES && e.src == ep.id)
        .collect();
    assert_eq!(
        exposes.len(),
        1,
        "expected exactly 1 EXPOSES edge for impl-method mcp_tool :EntryPoint"
    );
    assert_eq!(
        exposes[0].dst, expected_item,
        "EXPOSES dst must equal item_node_id(method_qname) for the impl method"
    );

    let entry_point_id = format!("entrypoint:mcp_tool:{expected_qname}");
    let register_edges: Vec<_> = edges
        .iter()
        .filter(|e| e.label.as_str() == EdgeLabel::REGISTERS_PARAM && e.src == entry_point_id)
        .collect();
    assert_eq!(
        register_edges.len(),
        2,
        "expected 2 REGISTERS_PARAM edges (x, y — self excluded); got {}: {:?}",
        register_edges.len(),
        register_edges
            .iter()
            .map(|e| (&e.src, &e.dst))
            .collect::<Vec<_>>(),
    );

    let mut dsts: Vec<&str> = register_edges.iter().map(|e| e.dst.as_str()).collect();
    dsts.sort();
    let expected_x = param_node_id(&expected_qname, 1);
    let expected_y = param_node_id(&expected_qname, 2);
    let expected_dsts = vec![expected_x.as_str(), expected_y.as_str()];
    assert_eq!(
        dsts, expected_dsts,
        "REGISTERS_PARAM dsts must equal param_node_id(method_qname, i) for receiver-offset \
         indices 1 and 2 — proves HIR-side dsts match syn-side :Param ids"
    );
}
