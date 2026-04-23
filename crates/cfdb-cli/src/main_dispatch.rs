//! Group-dispatch helpers for the `cfdb` CLI. Split out of `main.rs` as
//! part of the #128 god-file split. Each of the four helpers below
//! unpacks one slice of the `Command` enum and delegates to the
//! corresponding `cfdb_cli::*` handler.

use cfdb_cli::{
    check, check_predicate, diff, drop_keyspace_cmd, dump, enrich, export, extract, list_callers,
    list_items_matching, list_keyspaces, query, scope, snapshots, typed_stub, violations,
    CfdbCliError, EnrichVerb,
};

use crate::main_command::Command;

/// Dispatch helper for the INGEST + RAW + AUX core verbs. Factored out of
/// [`crate::run`] to keep the top-level match flat — each group's expansion
/// of the `cmd @ Command::*` alternation lives in a dedicated helper.
pub(crate) fn dispatch_core(cmd: Command) -> Result<(), CfdbCliError> {
    match cmd {
        Command::Extract {
            workspace,
            db,
            keyspace,
            hir,
            rev,
        } => extract(workspace, db, keyspace, hir, rev),
        Command::Query {
            db,
            keyspace,
            cypher,
            params,
            input,
        } => query(db, keyspace, cypher, params, input),
        Command::Violations {
            db,
            keyspace,
            rule,
            no_fail,
            count_only,
        } => {
            let rows_found = violations(db, keyspace, rule, count_only)?;
            if rows_found > 0 && !no_fail {
                std::process::exit(1);
            }
            Ok(())
        }
        Command::Check {
            db,
            keyspace,
            trigger,
            no_fail,
        } => {
            let rows_found = check(&db, &keyspace, trigger)?;
            if rows_found > 0 && !no_fail {
                std::process::exit(1);
            }
            Ok(())
        }
        Command::Dump { db, keyspace } => dump(db, keyspace),
        Command::Export {
            db,
            keyspace,
            format,
        } => export(db, keyspace, &format),
        Command::ListKeyspaces { db } => list_keyspaces(db),
        other => unreachable!("dispatch_core called with non-core command: {other:?}"),
    }
}

/// Dispatch helper for the TYPED verbs — the composer-over-Cypher
/// shortcuts. Same rationale as [`dispatch_core`].
pub(crate) fn dispatch_typed(cmd: Command) -> Result<(), CfdbCliError> {
    match cmd {
        Command::FindCanonical {
            db,
            keyspace,
            concept,
        } => typed_stub("find_canonical", &db, &keyspace, &[("concept", &concept)]),
        Command::ListCallers {
            db,
            keyspace,
            qname,
        } => list_callers(db, keyspace, qname),
        Command::ListBypasses {
            db,
            keyspace,
            concept,
        } => typed_stub("list_bypasses", &db, &keyspace, &[("concept", &concept)]),
        Command::ListItemsMatching {
            db,
            keyspace,
            name_pattern,
            kinds,
            group_by_context,
        } => list_items_matching(
            &db,
            &keyspace,
            &name_pattern,
            kinds.as_deref(),
            group_by_context,
        ),
        Command::Scope {
            db,
            context,
            workspace,
            format,
            output,
            keyspace,
            explain,
        } => scope(
            &db,
            &context,
            workspace.as_deref(),
            &format,
            output.as_deref(),
            keyspace.as_deref(),
            explain,
        ),
        Command::CheckPredicate {
            db,
            keyspace,
            workspace_root,
            name,
            params,
            format,
            no_fail,
        } => {
            let report = check_predicate(&db, &keyspace, &workspace_root, &name, &params)?;
            emit_check_predicate_report(&report, &format)?;
            if report.row_count > 0 && !no_fail {
                std::process::exit(1);
            }
            Ok(())
        }
        other => unreachable!("dispatch_typed called with non-typed command: {other:?}"),
    }
}

/// Render a [`cfdb_cli::PredicateRunReport`] to stdout per the `--format`
/// CLI arg. `text` emits a TSV-shaped `qname\tline\treason` per row plus
/// a stderr summary (same rhythm as `cfdb violations`); `json` emits a
/// pretty-printed report. Unknown formats are a [`CfdbCliError::Usage`].
fn emit_check_predicate_report(
    report: &cfdb_cli::PredicateRunReport,
    format: &str,
) -> Result<(), CfdbCliError> {
    match format {
        "text" => {
            eprintln!(
                "check-predicate: {} (predicate: {})",
                report.row_count, report.predicate_name
            );
            for row in &report.rows {
                println!("{}\t{}\t{}", row.qname, row.line, row.reason);
            }
            Ok(())
        }
        "json" => {
            let json = serde_json::to_string_pretty(&report)?;
            println!("{json}");
            Ok(())
        }
        other => Err(CfdbCliError::Usage(format!(
            "--format `{other}` not supported; expected `text` or `json`"
        ))),
    }
}

/// Dispatch helper for the SNAPSHOT verbs. Same rationale as
/// [`dispatch_core`].
pub(crate) fn dispatch_snapshot(cmd: Command) -> Result<(), CfdbCliError> {
    match cmd {
        Command::Snapshots { db } => snapshots(db),
        Command::Diff { db, a, b, kinds } => diff(db, a, b, kinds),
        Command::Drop { db, keyspace } => drop_keyspace_cmd(db, keyspace),
        other => unreachable!("dispatch_snapshot called with non-snapshot command: {other:?}"),
    }
}

/// Dispatch helper for the seven `Command::Enrich*` variants. Pulled out of
/// [`crate::run`] so each new enrichment verb does not balloon `run`'s
/// cyclomatic complexity — the top-level match collapses all seven arms to
/// a single alternation arm that delegates here.
pub(crate) fn dispatch_enrich(cmd: Command) -> Result<(), CfdbCliError> {
    // The git-history / rfc-docs / bounded-context verbs thread a workspace
    // path through the composition root (clean-arch B4 resolution, #43-A).
    // We handle them inline so the other four variants keep their simple
    // `(db, keyspace) → EnrichVerb` shape. Slice 43-F (#109) will add its
    // own `--workspace` flag when `enrich_concepts` needs one.
    if let Command::EnrichGitHistory {
        db,
        keyspace,
        workspace,
    } = cmd
    {
        return enrich(db, keyspace, EnrichVerb::GitHistory, workspace);
    }
    if let Command::EnrichRfcDocs {
        db,
        keyspace,
        workspace,
    } = cmd
    {
        return enrich(db, keyspace, EnrichVerb::RfcDocs, workspace);
    }
    if let Command::EnrichBoundedContext {
        db,
        keyspace,
        workspace,
    } = cmd
    {
        return enrich(db, keyspace, EnrichVerb::BoundedContext, workspace);
    }
    if let Command::EnrichConcepts {
        db,
        keyspace,
        workspace,
    } = cmd
    {
        return enrich(db, keyspace, EnrichVerb::Concepts, workspace);
    }
    if let Command::EnrichMetrics {
        db,
        keyspace,
        workspace,
    } = cmd
    {
        return enrich(db, keyspace, EnrichVerb::Metrics, workspace);
    }

    let (db, keyspace, verb) = match cmd {
        Command::EnrichDeprecation { db, keyspace } => (db, keyspace, EnrichVerb::Deprecation),
        Command::EnrichReachability { db, keyspace } => (db, keyspace, EnrichVerb::Reachability),
        other => {
            // Unreachable — the caller pattern-matches on the seven enrich
            // variants before calling us. An unexpected command here is a
            // dispatch-site bug, not an end-user error.
            unreachable!("dispatch_enrich called with non-enrich command: {other:?}")
        }
    };
    enrich(db, keyspace, verb, None)
}
