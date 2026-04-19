//! `cfdb scope` — structured §A3.3 infection inventory.
//!
//! Split out of `lib.rs` for the god-file decomposition (#3751). Public
//! surface preserved: every item here is re-exported from the crate root.

use std::path::Path;

use cfdb_core::result::{Warning, WarningKind};
use cfdb_core::schema::Keyspace;
use cfdb_core::store::StoreBackend;
use cfdb_core::{Param, PropValue, Query, RowValue};
use cfdb_petgraph::{persist, PetgraphStore};
use cfdb_query::{
    list_items_matching as compose_list_items_matching, parse, CanonicalCandidate, DebtClass,
    Finding, ScopeInventory,
};

use crate::commands::keyspace_path;

/// Embedded hsb-by-name rule, used by `cfdb scope` to seed
/// `canonical_candidates` from Pattern A horizontal split-brain findings.
const HSB_BY_NAME_CYPHER: &str = include_str!("../../../examples/queries/hsb-by-name.cypher");

/// `cfdb scope --context <name>` — emit the structured §A3.3 infection
/// inventory for a single bounded context (council-cfdb-wiring RATIFIED
/// §A.17). Pure data aggregation: no raid-plan markdown, no workflow
/// hints, no skill routing. Consumer skills (`/operate-module`,
/// `/boy-scout --from-inventory`) read the returned JSON and decide
/// what to do with it.
///
/// v0.1 population rules (per Forbidden move #10 / RFC-cfdb-v0.2-addendum
/// §A1.2):
/// - `findings_by_class`: 5 of 6 classes carry `[]` + a per-class
///   warning (their classifiers require the v0.2 pipeline or HIR-aware
///   emission). Only `canonical_bypass` populates today, via the shipping
///   `ledger-canonical-bypass.cypher` — and that rule is concept-specific
///   (ledger), not generic, so it surfaces as an empty bucket for every
///   other context plus a warning.
/// - `canonical_candidates`: seeded from `hsb-by-name.cypher` (Pattern A
///   horizontal split-brain candidates) filtered to the requested context.
/// - `reachability_map`: `None` (JSON `null`) — HIR-dependent per addendum
///   §A1.2. A warning is attached.
/// - `loc_per_crate`: approximated as `COUNT(:Item)` per `:Item.crate`
///   restricted to the requested context. True LOC requires
///   `cfdb-hir-extractor` (v0.2); a warning documents the approximation.
pub fn scope(
    db: &Path,
    context: &str,
    _workspace: Option<&Path>,
    format: &str,
    output: Option<&Path>,
    keyspace: Option<&str>,
) -> Result<(), crate::CfdbCliError> {
    if format != "json" {
        return Err(format!(
            "`--format {format}` is not supported in v0.1. \
             Only `json` ships today; `table` is deferred to v0.2 per §A3.3."
        )
        .into());
    }

    let ks_name = resolve_keyspace_name(db, keyspace)?;
    let ks = Keyspace::new(&ks_name);
    let ks_path = keyspace_path(db, &ks_name);
    if !ks_path.exists() {
        return Err(format!(
            "keyspace `{ks_name}` not found in db `{}` (looked for {})",
            db.display(),
            ks_path.display()
        )
        .into());
    }

    let mut store = PetgraphStore::new();
    persist::load(&mut store, &ks, &ks_path)?;

    // 1) Validate the context by enumerating `:Context{name}` nodes in the
    //    keyspace. cfdb-extractor emits one :Context per unique
    //    bounded_context at extract time (post-#3727 schema bump).
    let known_contexts = query_known_contexts(&store, &ks)?;
    if !known_contexts.iter().any(|c| c == context) {
        return Err(format!(
            "unknown context `{context}`; known contexts: [{}]",
            known_contexts.join(", ")
        )
        .into());
    }

    // 2) Pull every :Item in the bounded context. Reuse the typed composer
    //    from #3728 (pattern + no-kinds + no-grouping) and filter in Rust on
    //    `bounded_context` since the composer signature is name-pattern +
    //    kinds only. The keyspace-wide scan is acceptable in v0.1 — the
    //    composer returns flat rows with the 7 AC columns and we project
    //    Finding directly from them.
    let inventory_query = compose_list_items_matching(".*", None, false);
    let inventory_result = store.execute(&ks, &inventory_query)?;
    let mut findings_in_context: Vec<Finding> = Vec::with_capacity(inventory_result.rows.len());
    let mut loc_per_crate: std::collections::BTreeMap<String, u64> =
        std::collections::BTreeMap::new();
    for row in &inventory_result.rows {
        let row_context = scalar_str(row, "bounded_context").unwrap_or("");
        if row_context != context {
            continue;
        }
        if let Some(finding) = finding_from_row(row) {
            *loc_per_crate.entry(finding.crate_name.clone()).or_insert(0) += 1;
            findings_in_context.push(finding);
        }
    }
    findings_in_context.sort();

    // 3) Assemble the envelope. `keyspace_sha` is the keyspace name for
    //    now — the extract pipeline uses the keyspace name as its SHA
    //    anchor (sha is populated by Phase B snapshot wiring).
    let mut inventory = ScopeInventory::new(context, &ks_name);
    inventory.loc_per_crate = loc_per_crate;

    // 4) canonical_candidates from hsb-by-name rule, filtered to the
    //    requested bounded context (the rule itself is context-agnostic —
    //    we inspect each row's crates and include the candidate only if
    //    at least one crate belongs to the current context by cross-
    //    referencing the keyspace inventory).
    let hsb_parsed = parse(HSB_BY_NAME_CYPHER)
        .map_err(|e| format!("parse error in embedded hsb-by-name template: {e}"))?;
    let hsb_result = store.execute(&ks, &hsb_parsed)?;
    let crates_in_context = crates_for_context(&store, &ks, context)?;
    for row in &hsb_result.rows {
        if let Some(candidate) = canonical_candidate_from_row(row, &crates_in_context) {
            inventory.canonical_candidates.push(candidate);
        }
    }
    inventory.canonical_candidates.sort();

    // 5) Emit per-class warnings for every bucket whose v0.1 classifier
    //    is unavailable. Order mirrors DebtClass::variants() for G1
    //    determinism.
    for class in DebtClass::variants() {
        let note = class_v01_unavailable_note(*class);
        if let Some(message) = note {
            inventory.warnings.push(Warning {
                kind: WarningKind::EmptyResult,
                message,
                suggestion: None,
            });
        }
    }

    // 6) reachability_map HIR degradation warning.
    inventory.warnings.push(Warning {
        kind: WarningKind::EmptyResult,
        message: "`reachability_map` is `null` in v0.1 — CALLS / :CallSite edges \
                  require cfdb-hir-extractor (addendum §A1.2); ships in v0.2."
            .to_string(),
        suggestion: None,
    });

    // 7) loc_per_crate approximation warning — only emit when we actually
    //    populated counts (silence when the context has no items, which is
    //    already signalled via empty-class warnings).
    if !inventory.loc_per_crate.is_empty() {
        inventory.warnings.push(Warning {
            kind: WarningKind::EmptyResult,
            message: "`loc_per_crate` reports :Item count per crate, not true \
                      lines-of-code (LOC requires cfdb-hir-extractor — v0.2)."
                .to_string(),
            suggestion: None,
        });
    }

    let json = serde_json::to_string_pretty(&inventory)?;
    match output {
        Some(path) => {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("create output parent dir `{}`: {e}", parent.display()))?;
            }
            std::fs::write(path, &json)
                .map_err(|e| format!("write output file `{}`: {e}", path.display()))?;
        }
        None => {
            println!("{json}");
        }
    }
    Ok(())
}

/// Resolve the keyspace name to query for `cfdb scope`. If the caller
/// supplied `--keyspace`, use it. Otherwise, if the db directory holds
/// exactly one `.json` keyspace file, use its stem. Any other case is a
/// usage error — the user must disambiguate.
fn resolve_keyspace_name(db: &Path, keyspace: Option<&str>) -> Result<String, crate::CfdbCliError> {
    if let Some(name) = keyspace {
        return Ok(name.to_string());
    }
    if !db.exists() {
        return Err(format!("db directory `{}` does not exist", db.display()).into());
    }
    let mut names: Vec<String> = std::fs::read_dir(db)?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) == Some("json") {
                p.file_stem().and_then(|s| s.to_str()).map(String::from)
            } else {
                None
            }
        })
        .collect();
    names.sort();
    match names.len() {
        0 => Err(format!(
            "db `{}` contains no keyspace files; run `cfdb extract` first",
            db.display()
        )
        .into()),
        1 => Ok(names.into_iter().next().expect("len==1 — just checked")),
        n => Err(format!(
            "db `{}` contains {n} keyspaces; pass --keyspace to disambiguate",
            db.display()
        )
        .into()),
    }
}

/// Run `MATCH (c:Context) RETURN c.name` and collect the sorted list.
fn query_known_contexts(
    store: &PetgraphStore,
    ks: &Keyspace,
) -> Result<Vec<String>, crate::CfdbCliError> {
    use cfdb_core::query::{NodePattern, Pattern, ProjectionValue};
    use cfdb_core::{Expr, Projection, ReturnClause};
    use std::collections::BTreeMap;
    let q = Query {
        match_clauses: vec![Pattern::Node(NodePattern {
            var: Some("c".to_string()),
            label: Some(cfdb_core::Label::new(cfdb_core::Label::CONTEXT)),
            props: BTreeMap::new(),
        })],
        where_clause: None,
        with_clause: None,
        return_clause: ReturnClause {
            projections: vec![Projection {
                value: ProjectionValue::Expr(Expr::Property {
                    var: "c".to_string(),
                    prop: "name".to_string(),
                }),
                alias: Some("name".to_string()),
            }],
            order_by: vec![cfdb_core::OrderBy {
                expr: Expr::Var("name".to_string()),
                descending: false,
            }],
            limit: None,
            distinct: false,
        },
        params: BTreeMap::new(),
    };
    let result = store.execute(ks, &q)?;
    let mut names: Vec<String> = result
        .rows
        .iter()
        .filter_map(|r| scalar_str(r, "name").map(String::from))
        .collect();
    names.sort();
    names.dedup();
    Ok(names)
}

/// Enumerate every `:Item.crate` whose `bounded_context` equals the
/// requested context. Used to filter `hsb-by-name` candidate rows.
fn crates_for_context(
    store: &PetgraphStore,
    ks: &Keyspace,
    context: &str,
) -> Result<std::collections::BTreeSet<String>, crate::CfdbCliError> {
    use cfdb_core::query::{NodePattern, Pattern, ProjectionValue};
    use cfdb_core::{CompareOp, Expr, Predicate, Projection, ReturnClause};
    use std::collections::BTreeMap;
    let q = Query {
        match_clauses: vec![Pattern::Node(NodePattern {
            var: Some("i".to_string()),
            label: Some(cfdb_core::Label::new(cfdb_core::Label::ITEM)),
            props: BTreeMap::new(),
        })],
        where_clause: Some(Predicate::Compare {
            left: Expr::Property {
                var: "i".to_string(),
                prop: "bounded_context".to_string(),
            },
            op: CompareOp::Eq,
            right: Expr::Param("context".to_string()),
        }),
        with_clause: None,
        return_clause: ReturnClause {
            projections: vec![Projection {
                value: ProjectionValue::Expr(Expr::Property {
                    var: "i".to_string(),
                    prop: "crate".to_string(),
                }),
                alias: Some("crate".to_string()),
            }],
            order_by: vec![],
            limit: None,
            distinct: true,
        },
        params: {
            let mut m = BTreeMap::new();
            m.insert(
                "context".to_string(),
                Param::Scalar(PropValue::Str(context.to_string())),
            );
            m
        },
    };
    let result = store.execute(ks, &q)?;
    Ok(result
        .rows
        .iter()
        .filter_map(|r| scalar_str(r, "crate").map(String::from))
        .collect())
}

fn scalar_str<'a>(row: &'a cfdb_core::Row, column: &str) -> Option<&'a str> {
    row.get(column).and_then(|v| v.as_str())
}

fn finding_from_row(row: &cfdb_core::Row) -> Option<Finding> {
    Some(Finding {
        qname: scalar_str(row, "qname")?.to_string(),
        name: scalar_str(row, "name")?.to_string(),
        kind: scalar_str(row, "kind")?.to_string(),
        crate_name: scalar_str(row, "crate")?.to_string(),
        file: scalar_str(row, "file")?.to_string(),
        line: row.get("line").and_then(|v| v.as_i64()).unwrap_or(0) as u64,
        bounded_context: scalar_str(row, "bounded_context")?.to_string(),
    })
}

fn row_list_str(row: &cfdb_core::Row, column: &str) -> Vec<String> {
    row.get(column)
        .and_then(|v| match v {
            RowValue::List(xs) => Some(
                xs.iter()
                    .filter_map(|p| p.as_str().map(str::to_string))
                    .collect(),
            ),
            _ => None,
        })
        .unwrap_or_default()
}

fn canonical_candidate_from_row(
    row: &cfdb_core::Row,
    crates_in_context: &std::collections::BTreeSet<String>,
) -> Option<CanonicalCandidate> {
    let crates = row_list_str(row, "crates");
    // Retain only candidates whose crate set intersects the current context.
    // A candidate with no context-owned crates is a "neighbour" finding that
    // belongs to a different context's inventory.
    if !crates.iter().any(|c| crates_in_context.contains(c)) {
        return None;
    }
    Some(CanonicalCandidate {
        name: scalar_str(row, "name")?.to_string(),
        kind: scalar_str(row, "kind")?.to_string(),
        crates,
        qnames: row_list_str(row, "qnames"),
        files: row_list_str(row, "files"),
    })
}

/// Message for each `DebtClass` bucket whose v0.1 classifier is unavailable.
/// Returns `None` for classes that DO populate in v0.1 (none today beyond
/// the hsb-sourced `canonical_candidates` list).
fn class_v01_unavailable_note(class: DebtClass) -> Option<String> {
    let reason = match class {
        DebtClass::DuplicatedFeature => {
            "duplicated_feature requires the v0.2 classifier (addendum §A2.2) \
             to distinguish same-context duplication from cross-context homonyms; \
             hsb-by-name candidates are surfaced as canonical_candidates instead"
        }
        DebtClass::ContextHomonym => {
            "context_homonym requires the v0.2 classifier with bounded-context \
             cross-reference (addendum §A2.1); blocked on Pattern I completeness"
        }
        DebtClass::UnfinishedRefactor => {
            "unfinished_refactor requires the v0.2 classifier; the v0.1 ruleset \
             ships no rule that emits this class label"
        }
        DebtClass::RandomScattering => {
            "random_scattering requires Pattern B (vertical call-chain analysis) \
             which is HIR-blocked (addendum §A1.2)"
        }
        DebtClass::CanonicalBypass => {
            "canonical_bypass: only the concept-specific `ledger-canonical-bypass` \
             rule ships in v0.1; a generic bypass classifier is v0.2 work"
        }
        DebtClass::Unwired => {
            "unwired requires :EntryPoint + CALLS edges (HIR-blocked, addendum §A1.2)"
        }
    };
    Some(format!(
        "findings_by_class.{class} is empty in v0.1 — {reason}"
    ))
}
