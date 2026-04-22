//! `cfdb scope` — structured §A3.3 infection inventory.
//!
//! Split out of `lib.rs` for the god-file decomposition (#3751). Public
//! surface preserved: every item here is re-exported from the crate root.

use std::path::Path;

use cfdb_core::result::{Warning, WarningKind};
use cfdb_query::{DebtClass, ScopeInventory};

use crate::commands::keyspace_path;
use crate::compose;

mod classifier;
mod explain_sink;
mod helpers;

use classifier::{query_canonical_candidates, query_findings_in_context, run_classifier_rule};
use explain_sink::ExplainSink;
use helpers::validate_context;

/// Embedded hsb-by-name rule, used by `cfdb scope` to seed
/// `canonical_candidates` from Pattern A horizontal split-brain findings.
const HSB_BY_NAME_CYPHER: &str = include_str!("../../../examples/queries/hsb-by-name.cypher");

/// Embedded classifier rules (issue #48, §A2.1 six-class taxonomy).
///
/// Each rule projects `Finding`-compatible columns (qname, name, kind,
/// crate, file, line, bounded_context) and accepts a single `$context`
/// parameter. The CLI orchestrator runs them per-class when assembling
/// `ScopeInventory::findings_by_class`.
const CLASSIFIER_DUPLICATED_FEATURE_CYPHER: &str =
    include_str!("../../../examples/queries/classifier-duplicated-feature.cypher");
const CLASSIFIER_CONTEXT_HOMONYM_CYPHER: &str =
    include_str!("../../../examples/queries/classifier-context-homonym.cypher");
const CLASSIFIER_UNFINISHED_REFACTOR_CYPHER: &str =
    include_str!("../../../examples/queries/classifier-unfinished-refactor.cypher");
const CLASSIFIER_RANDOM_SCATTERING_CYPHER: &str =
    include_str!("../../../examples/queries/classifier-random-scattering.cypher");
const CLASSIFIER_CANONICAL_BYPASS_CYPHER: &str =
    include_str!("../../../examples/queries/classifier-canonical-bypass.cypher");
const CLASSIFIER_UNWIRED_CYPHER: &str =
    include_str!("../../../examples/queries/classifier-unwired.cypher");

/// `cfdb scope --context <name>` — emit the structured §A3.3 infection
/// inventory for a single bounded context (council-cfdb-wiring RATIFIED
/// §A.17). Pure data aggregation: no raid-plan markdown, no workflow
/// hints, no skill routing. Consumer skills (`/operate-module`,
/// `/boy-scout --from-inventory`) read the returned JSON and decide
/// what to do with it.
///
/// v0.1 population rules (issue #48 wires all 6 classifier rules;
/// addendum §A2.1 + §A2.2):
/// - `findings_by_class`: each of the six `DebtClass` buckets is
///   populated by a dedicated classifier rule in
///   `examples/queries/classifier-*.cypher`. Rules that require HIR-
///   extracted inputs (`ContextHomonym`, `RandomScattering`,
///   `CanonicalBypass`, `Unwired`) return empty rows when the keyspace
///   was built without `--features hir`; a per-class warning documents
///   the degradation. Rules whose inputs are always present
///   (`DuplicatedFeature`, `UnfinishedRefactor`) never degrade.
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
    workspace: Option<&Path>,
    format: &str,
    output: Option<&Path>,
    keyspace: Option<&str>,
    explain: bool,
) -> Result<(), crate::CfdbCliError> {
    if format != "json" {
        return Err(format!(
            "`--format {format}` is not supported in v0.1. \
             Only `json` ships today; `table` is deferred to v0.2 per §A3.3."
        )
        .into());
    }

    let ks_name = resolve_keyspace_name(db, keyspace)?;
    let ks_path = keyspace_path(db, &ks_name);
    if !ks_path.exists() {
        return Err(format!(
            "keyspace `{ks_name}` not found in db `{}` (looked for {})",
            db.display(),
            ks_path.display()
        )
        .into());
    }

    // RFC-035 §3.8: when a workspace is supplied, route through
    // `load_store_with_workspace` so `.cfdb/indexes.toml` flows into the
    // store and the slice-5/6 fast paths activate. Without a workspace,
    // fall back to the index-free loader for backward compat.
    let (store, ks) = match workspace {
        Some(ws) => compose::load_store_with_workspace(db, &ks_name, Some(ws.to_path_buf()))?,
        None => compose::load_store(db, &ks_name)?,
    };
    validate_context(&store, &ks, context)?;
    let sink = if explain {
        ExplainSink::enabled()
    } else {
        ExplainSink::disabled()
    };
    let inventory = build_scope_inventory(&store, &ks, context, &ks_name, &sink)?;
    if explain {
        for row in sink.drain() {
            eprintln!("{}", row.format_line());
        }
    }
    emit_scope_output(&inventory, output)
}

/// Assemble the full `ScopeInventory` for the requested context — items,
/// canonical candidates, warnings. Pulled out of [`scope`] so the sequence
/// of "query → filter → attach warnings" lives in a dedicated body with
/// its own complexity budget.
fn build_scope_inventory(
    store: &cfdb_petgraph::PetgraphStore,
    ks: &cfdb_core::schema::Keyspace,
    context: &str,
    ks_name: &str,
    sink: &ExplainSink,
) -> Result<ScopeInventory, crate::CfdbCliError> {
    let (findings_in_context, loc_per_crate) = query_findings_in_context(store, ks, context, sink)?;

    let mut inventory = ScopeInventory::new(context, ks_name);
    inventory.loc_per_crate = loc_per_crate;
    let _ = findings_in_context; // reserved for future inventory population — see §A3.3

    inventory.canonical_candidates = query_canonical_candidates(store, ks, context, sink)?;
    inventory.canonical_candidates.sort();

    // Issue #48 — populate each class bucket via its classifier rule.
    populate_findings_by_class(store, ks, context, &mut inventory, sink)?;

    attach_scope_warnings(&mut inventory);
    Ok(inventory)
}

/// Run each classifier rule (§A2.1 six classes) and fill the
/// corresponding bucket in `inventory.findings_by_class`. Rules that
/// return an empty row set — either because no finding exists OR
/// because the required enrichment pass (HIR, concepts, reachability)
/// was not run against the keyspace — leave the bucket empty; the
/// warning path in [`attach_scope_warnings`] reports dependency
/// degradations.
fn populate_findings_by_class(
    store: &cfdb_petgraph::PetgraphStore,
    ks: &cfdb_core::schema::Keyspace,
    context: &str,
    inventory: &mut ScopeInventory,
    sink: &ExplainSink,
) -> Result<(), crate::CfdbCliError> {
    for (class, cypher) in classifier::classifier_rules() {
        let findings = run_classifier_rule(store, ks, context, cypher, sink)?;
        if let Some(bucket) = inventory.findings_by_class.get_mut(&class) {
            bucket.extend(findings);
            bucket.sort();
            bucket.dedup();
        }
    }
    Ok(())
}

/// Attach the full warning set for a `cfdb scope` inventory — per-class
/// dependency / degradation notes (only when the bucket is empty), the
/// reachability-map HIR caveat, and the loc-per-crate approximation
/// note. Split out of [`build_scope_inventory`] to keep the assembly
/// body flat.
///
/// Issue #48: classes that produced at least one finding do NOT get a
/// warning — the bucket itself is the signal. Empty buckets carry a
/// warning naming the likely cause (missing enrichment, no signal in
/// this context, etc.) so consumers can distinguish "zero bypass bugs"
/// from "reachability enrichment was not run".
fn attach_scope_warnings(inventory: &mut ScopeInventory) {
    DebtClass::variants()
        .iter()
        .filter(|class| {
            inventory
                .findings_by_class
                .get(class)
                .map(|v| v.is_empty())
                .unwrap_or(true)
        })
        .filter_map(|class| class_empty_bucket_note(*class))
        .for_each(|message| {
            inventory.warnings.push(Warning {
                kind: WarningKind::EmptyResult,
                message,
                suggestion: None,
            });
        });
    inventory.warnings.push(Warning {
        kind: WarningKind::EmptyResult,
        message: "`reachability_map` is `null` in v0.1 — CALLS / :CallSite edges \
                  require cfdb-hir-extractor (addendum §A1.2); ships in v0.2."
            .to_string(),
        suggestion: None,
    });
    if !inventory.loc_per_crate.is_empty() {
        inventory.warnings.push(Warning {
            kind: WarningKind::EmptyResult,
            message: "`loc_per_crate` reports :Item count per crate, not true \
                      lines-of-code (LOC requires cfdb-hir-extractor — v0.2)."
                .to_string(),
            suggestion: None,
        });
    }
}

/// Serialise the inventory and write it to `output` (or stdout if `None`).
fn emit_scope_output(
    inventory: &ScopeInventory,
    output: Option<&Path>,
) -> Result<(), crate::CfdbCliError> {
    let json = serde_json::to_string_pretty(inventory)?;
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

/// Diagnostic for a `DebtClass` whose bucket is empty after the
/// classifier run. Names the likely degraded input that would cause
/// a false negative — a keyspace extracted without the required
/// enrichment pass. For classes whose inputs are always present in a
/// syn-only extract (`DuplicatedFeature`, `UnfinishedRefactor`), the
/// message reports the empty result as "no finding in this context"
/// rather than a dependency gap.
///
/// Issue #48 replaces the v0.1-style "classifier unavailable" note
/// with per-class degradation semantics now that each classifier
/// rule ships. The class name still appears in every message so
/// consumers can grep for a specific class.
fn class_empty_bucket_note(class: DebtClass) -> Option<String> {
    let reason = match class {
        DebtClass::DuplicatedFeature => {
            "findings_by_class.duplicated_feature is empty — no same-context \
             struct/enum/trait homonyms in this context (inputs: :Item.name, \
             :Item.bounded_context — always present in a syn-only extract)"
        }
        DebtClass::ContextHomonym => {
            "findings_by_class.context_homonym is empty — no cross-context \
             signature-divergent fn/method pairs in this context. If the \
             keyspace was extracted without --features hir, :Item.signature \
             is absent and this class degrades to no findings; run `cfdb \
             extract --features hir` to enable."
        }
        DebtClass::UnfinishedRefactor => {
            "findings_by_class.unfinished_refactor is empty — no \
             #[deprecated] items in this context (inputs: :Item.is_deprecated, \
             :Item.bounded_context — always present in a syn-only extract)"
        }
        DebtClass::RandomScattering => {
            "findings_by_class.random_scattering is empty — no Pattern B \
             fork findings in this context. If the keyspace was extracted \
             without --features hir, :EntryPoint nodes and CALLS edges are \
             absent and this class degrades to no findings; run `cfdb \
             extract --features hir` to enable."
        }
        DebtClass::CanonicalBypass => {
            "findings_by_class.canonical_bypass is empty — no CANONICAL_FOR \
             unreachable items in this context. Requires both `cfdb \
             enrich-concepts` (CANONICAL_FOR edges from .cfdb/concepts/*.toml) \
             AND `cfdb enrich-reachability` (reachable_from_entry attr, \
             HIR-dependent). Concept-specific BYPASS_REACHABLE / BYPASS_DEAD \
             rules remain available for per-concept triage."
        }
        DebtClass::Unwired => {
            "findings_by_class.unwired is empty — no unreachable fn/method \
             items in this context. Requires `cfdb enrich-reachability` \
             (HIR-dependent). On a keyspace without HIR, every fn is \
             trivially unreachable in the graph's view; the classifier \
             therefore returns empty rather than flooding with false \
             positives."
        }
    };
    Some(reason.to_string())
}
