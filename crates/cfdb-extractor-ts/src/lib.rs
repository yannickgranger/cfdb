//! `cfdb-extractor-ts` — TypeScript-language `LanguageProducer` MVP
//! (RFC-041 Phase 3 / issue #265 / META #266).
//!
//! Walks a Next.js-shaped TS project syntactically via
//! [`tree-sitter-typescript`] and emits the v0.1 cfdb fact set
//! (`:Crate`, `:Module`, `:Item`, plus `IN_CRATE` / `IN_MODULE`
//! edges). Pairs with `cfdb-extractor` (Rust reference impl) and the
//! follow-up `cfdb-extractor-php` (#264) — all three plug into the
//! same `cfdb-cli` dispatcher per RFC-041 §3.4.
//!
//! # TS → Rust closed-set `:Item.kind` mapping (load-bearing)
//!
//! `cfdb-core::schema::labels` declares a closed set of `:Item.kind`
//! values (`"struct"`, `"enum"`, `"trait"`, `"fn"`, `"impl_block"`,
//! `"const"`, `"static"`, `"type"`, `"mod"`). Per RFC-041 §4
//! Published Language invariant, this producer MUST NOT emit a
//! `kind` outside that set — adding a TypeScript-native value
//! (`"interface"`, `"type_alias"`, `"class"`, `"namespace"`,
//! `"jsx_component"`) requires a separate schema RFC + a
//! `cfdb-core::SchemaVersion` patch + a lockstep PR on
//! `graph-specs-rust` per RFC-033 §4 I2. This crate works around the
//! constraint by mapping each TS construct to its closest semantic
//! Rust analogue:
//!
//! | TS construct                        | Mapped to                          | Rationale                                                                 |
//! | ----------------------------------- | ---------------------------------- | ------------------------------------------------------------------------- |
//! | TS `module` (one per `.ts` file)    | `:Module` node (NOT `:Item.namespace`) | TS files behave like Rust file-modules; reuse the existing `:Module` label. |
//! | TS `interface_declaration`          | `:Item { kind: "trait" }`          | Both declare a contract (set of method/property signatures) without impl. |
//! | TS `type_alias_declaration`         | `:Item { kind: "type" }`           | Exact semantic match — already in the closed set.                          |
//! | TS `class_declaration`              | `:Item { kind: "struct" }`         | TS classes are stateful aggregates; closest to Rust struct + impl block.  |
//! | TS exported `function_declaration`  | `:Item { kind: "fn" }`             | Exact semantic match.                                                     |
//! | TS top-level `const` / `let` / `var`| `:Item { kind: "const" }`          | Closed set has no `let`/`var`; collapse the three into `const` on emit.    |
//!
//! Non-exported items at the top level are still emitted (TS does
//! not require an explicit `pub` to reach module scope, unlike Rust)
//! but carry `visibility: "private"`. Exported items carry
//! `visibility: "public"`. JSX components and `.tsx` files are out
//! of scope for the MVP — see `LANGUAGE_TYPESCRIPT` (NOT
//! `LANGUAGE_TSX`) below.
//!
//! # Determinism
//!
//! The walker emits files in alphabetical order (BTreeSet iteration)
//! and items in source order within a file. The trait method sorts
//! the final `(nodes, edges)` tuples canonically before returning,
//! matching `cfdb-extractor`'s convention so downstream
//! `cfdb-petgraph` ingestion hashes the same regardless of who
//! produced the bytes. Two consecutive `produce()` calls on the same
//! tree are byte-identical.
//!
//! # AC bar (RFC-041 §7 Phase 3 / issue #265)
//!
//! The MVP intentionally does NOT match `ts-morph` parity. The AC
//! is satisfied if the fixture round-trips at least one
//! `:Item { kind: "trait" }` (interface), one
//! `:Item { kind: "type" }` (type alias), and one
//! `:Item { kind: "fn" }` (exported function). Cross-file imports,
//! re-exports, JSX, decorators, generics, and namespace-merging are
//! deferred to a follow-up RFC.

use std::fs;
use std::path::{Path, PathBuf};

use cfdb_core::fact::{Edge, Node, PropValue, Props};
use cfdb_core::schema::Label;
use cfdb_lang::{LanguageError, LanguageProducer};
use tree_sitter::Parser;

mod emit;

/// Stable producer identifier — matches the `lang-typescript` Cargo
/// feature gate on `cfdb-cli` (RFC-041 §3.4) and the keyspace suffix
/// `cfdb-cli` derives at dispatch time.
const PRODUCER_NAME: &str = "typescript";

/// Workspace-root marker filenames. BOTH must be present for a
/// directory to be detected as a TypeScript project — `package.json`
/// alone matches plain Node.js / JavaScript projects, which this
/// producer does not handle. The `tsconfig.json` requirement is the
/// TS-specific signal (RFC-041 §3.4 detection contract).
const TSCONFIG_JSON: &str = "tsconfig.json";
const PACKAGE_JSON: &str = "package.json";

/// Directory names skipped by the file walker. `node_modules` would
/// pull tens of thousands of vendored `.ts` files into a single
/// extract — explicitly excluded. `dist` and `build` are common
/// compiled-output directories that contain transpiled `.ts` (or
/// `.d.ts`) we do not want to double-emit.
const SKIPPED_DIRS: &[&str] = &["node_modules", "dist", "build"];

/// TypeScript reference implementation of `cfdb_lang::LanguageProducer`
/// (RFC-041 Phase 3 / issue #265).
///
/// See the crate-root docs for the TS → Rust closed-set mapping and
/// the explicit AC bar.
pub struct TypeScriptProducer;

impl LanguageProducer for TypeScriptProducer {
    fn name(&self) -> &'static str {
        PRODUCER_NAME
    }

    fn detect(&self, workspace_root: &Path) -> bool {
        workspace_root.join(TSCONFIG_JSON).is_file() && workspace_root.join(PACKAGE_JSON).is_file()
    }

    fn produce(&self, workspace_root: &Path) -> Result<(Vec<Node>, Vec<Edge>), LanguageError> {
        if !self.detect(workspace_root) {
            return Err(LanguageError::NotDetected {
                producer: PRODUCER_NAME,
                reason: format!(
                    "missing `{TSCONFIG_JSON}` and/or `{PACKAGE_JSON}` at workspace root"
                ),
            });
        }

        let crate_name = derive_crate_name(workspace_root);
        let crate_id = format!("crate:{crate_name}");

        let mut nodes: Vec<Node> = Vec::new();
        let mut edges: Vec<Edge> = Vec::new();

        // :Crate — one per TS workspace. The `is_workspace_member`
        // prop matches what `cfdb-extractor`'s Rust path emits; the
        // `published_language` prop is `false` for TS workspaces by
        // default (the marker file `.cfdb/published-language-crates.toml`
        // applies to Rust crate names, not TS package names).
        nodes.push(Node {
            id: crate_id.clone(),
            label: Label::new(Label::CRATE),
            props: {
                let mut p = Props::new();
                p.insert("name".into(), PropValue::Str(crate_name.clone()));
                p.insert("language".into(), PropValue::Str(PRODUCER_NAME.into()));
                p.insert("is_workspace_member".into(), PropValue::Bool(true));
                p.insert("published_language".into(), PropValue::Bool(false));
                p
            },
        });

        let ts_files = collect_ts_files(workspace_root).map_err(LanguageError::Io)?;

        let mut parser = Parser::new();
        let language: tree_sitter::Language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
        parser
            .set_language(&language)
            .map_err(|e| LanguageError::Parse {
                producer: PRODUCER_NAME,
                message: format!("set_language(LANGUAGE_TYPESCRIPT): {e}"),
            })?;

        for file_path in &ts_files {
            walk_file(
                &mut parser,
                workspace_root,
                file_path,
                &crate_name,
                &crate_id,
                &mut nodes,
                &mut edges,
            )?;
        }

        // Canonical sort — matches cfdb-extractor's contract so two
        // producers' output streams compose deterministically when
        // ingested into the same `:Crate`-disjoint keyspace.
        nodes.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));
        edges.sort_by(|a, b| a.sort_key().cmp(&b.sort_key()));

        Ok((nodes, edges))
    }
}

/// Derive a synthetic crate name from the workspace root. We do NOT
/// parse `package.json` here — the MVP keeps the dep tree minimal
/// (no `serde_json` in this crate) and uses the directory's last
/// path segment as the crate name. A follow-up can read
/// `package.json#name` once we have a justification for taking on
/// the dep.
fn derive_crate_name(workspace_root: &Path) -> String {
    workspace_root
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("ts_workspace")
        .to_string()
}

/// Recursive walk of `workspace_root`, collecting every `*.ts` file
/// (NOT `*.tsx` — out of scope for the MVP). Skips `SKIPPED_DIRS`
/// and any path that is not a real file. Output is sorted by path
/// string for deterministic emission order.
fn collect_ts_files(workspace_root: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut acc = Vec::new();
    visit_dir(workspace_root, &mut acc)?;
    acc.sort();
    Ok(acc)
}

fn visit_dir(dir: &Path, acc: &mut Vec<PathBuf>) -> std::io::Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e),
    };
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            let dir_name = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            if SKIPPED_DIRS.contains(&dir_name) || dir_name.starts_with('.') {
                continue;
            }
            visit_dir(&path, acc)?;
        } else if file_type.is_file()
            && path
                .extension()
                .and_then(|s| s.to_str())
                .map(|ext| ext == "ts")
                .unwrap_or(false)
            && !path
                .file_name()
                .and_then(|s| s.to_str())
                .map(|n| n.ends_with(".d.ts"))
                .unwrap_or(false)
        {
            acc.push(path);
        }
    }
    Ok(())
}

/// Parse one `.ts` file and emit its `:Module` node + child `:Item`
/// nodes + structural edges.
fn walk_file(
    parser: &mut Parser,
    workspace_root: &Path,
    file_path: &Path,
    crate_name: &str,
    crate_id: &str,
    nodes: &mut Vec<Node>,
    edges: &mut Vec<Edge>,
) -> Result<(), LanguageError> {
    let source = fs::read_to_string(file_path).map_err(LanguageError::Io)?;
    let tree = parser.parse(&source, None).ok_or(LanguageError::Parse {
        producer: PRODUCER_NAME,
        message: format!("tree-sitter returned None for {}", file_path.display()),
    })?;

    let rel_path = file_path
        .strip_prefix(workspace_root)
        .unwrap_or(file_path)
        .to_string_lossy()
        .to_string();
    // Module qpath: dotted form of the relative path with `.ts`
    // stripped, mirroring how Rust modules become `crate::foo::bar`
    // in cfdb's qname grammar. `src/user.ts` → `src.user`.
    let module_qpath = ts_module_qpath(&rel_path);
    let module_id = format!("module:{crate_name}::{module_qpath}");

    nodes.push(Node {
        id: module_id.clone(),
        label: Label::new(Label::MODULE),
        props: {
            let mut p = Props::new();
            p.insert("qpath".into(), PropValue::Str(module_qpath.clone()));
            p.insert("file".into(), PropValue::Str(rel_path.clone()));
            p.insert("crate".into(), PropValue::Str(crate_name.to_string()));
            p
        },
    });

    let root = tree.root_node();
    let bytes = source.as_bytes();
    emit::walk_program(
        root,
        bytes,
        crate_name,
        crate_id,
        &module_qpath,
        &module_id,
        &rel_path,
        nodes,
        edges,
    );
    Ok(())
}

/// Convert a relative file path (`src/user.ts`) to a dotted module
/// qpath (`src.user`). Path separators collapse to `.`; the trailing
/// `.ts` extension is stripped.
fn ts_module_qpath(rel_path: &str) -> String {
    let trimmed = rel_path.strip_suffix(".ts").unwrap_or(rel_path);
    trimmed
        .split(['/', '\\'])
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(".")
}
