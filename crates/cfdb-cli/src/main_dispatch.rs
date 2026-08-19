use std::str::FromStr;

use crate::main_exit::findings_exit;
#[cfg(feature = "classify")]
use cfdb_cli::{check, classify, scope, typed_stub};
use cfdb_cli::{
    check_predicate, diff, drop_keyspace_cmd, dump, emit_json, enrich, export, extract, impact,
    list_callers, list_items_matching, list_keyspaces, query, snapshots, violations, CfdbCliError,
    EnrichVerb, OutputFormat,
};

use crate::main_command::{Command, ExtractArgs};

pub(crate) fn dispatch_core(cmd: Command) -> Result<(), CfdbCliError> {
    match cmd {
        Command::Extract(ExtractArgs {
            workspace,
            db,
            keyspace,
            hir,
            no_proc_macro,
            rev,
            profile,
        }) => extract(workspace, db, keyspace, hir, no_proc_macro, rev, profile),
        Command::Query(args) => query(args.db, args.keyspace, args.cypher, args.params, args.input),
        Command::Violations {
            db,
            keyspace,
            rule,
            no_fail,
            count_only,
        } => {
            let rows_found = violations(db, keyspace, rule, count_only)?;
            if rows_found > 0 && !no_fail {
                findings_exit();
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

pub(crate) fn dispatch_typed(cmd: Command) -> Result<(), CfdbCliError> {
    match cmd {
        Command::ListCallers {
            db,
            keyspace,
            qname,
        } => list_callers(db, keyspace, qname),
        Command::Impact(args) => impact(
            args.db,
            args.keyspace,
            args.item,
            args.since,
            args.workspace,
            args.max_depth,
        ),
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
                findings_exit();
            }
            Ok(())
        }
        other => unreachable!("dispatch_typed called with non-typed command: {other:?}"),
    }
}

fn emit_check_predicate_report(
    report: &cfdb_cli::PredicateRunReport,
    format: &str,
) -> Result<(), CfdbCliError> {
    let format = OutputFormat::from_str(format)?
        .require_one_of(&[OutputFormat::Text, OutputFormat::Json], "check-predicate")?;
    match format {
        OutputFormat::Text => {
            eprintln!(
                "check-predicate: {} (predicate: {})",
                report.row_count, report.predicate_name
            );
            for row in &report.rows {
                println!("{}\t{}\t{}", row.qname, row.line, row.reason);
            }
            Ok(())
        }
        OutputFormat::Json => emit_json(&report),
        _ => unreachable!("check-predicate allowlist is restricted to Text | Json"),
    }
}

pub(crate) fn dispatch_snapshot(cmd: Command) -> Result<(), CfdbCliError> {
    match cmd {
        Command::Snapshots { db } => snapshots(db),
        Command::Diff {
            db,
            a,
            b,
            kinds,
            format,
        } => diff(db, a, b, kinds, format),
        Command::Drop { db, keyspace } => drop_keyspace_cmd(db, keyspace),
        other => unreachable!("dispatch_snapshot called with non-snapshot command: {other:?}"),
    }
}

pub(crate) fn dispatch_enrich(cmd: Command) -> Result<(), CfdbCliError> {
    let (db, keyspace, verb, workspace) = match cmd {
        Command::EnrichGitHistory {
            db,
            keyspace,
            workspace,
        } => (db, keyspace, EnrichVerb::GitHistory, workspace),
        Command::EnrichRfcDocs {
            db,
            keyspace,
            workspace,
        } => (db, keyspace, EnrichVerb::RfcDocs, workspace),
        Command::EnrichBoundedContext {
            db,
            keyspace,
            workspace,
        } => (db, keyspace, EnrichVerb::BoundedContext, workspace),
        Command::EnrichConcepts {
            db,
            keyspace,
            workspace,
        } => (db, keyspace, EnrichVerb::Concepts, workspace),
        Command::EnrichMetrics {
            db,
            keyspace,
            workspace,
        } => (db, keyspace, EnrichVerb::Metrics, workspace),
        Command::EnrichDeprecation { db, keyspace } => {
            (db, keyspace, EnrichVerb::Deprecation, None)
        }
        Command::EnrichReachability { db, keyspace } => {
            (db, keyspace, EnrichVerb::Reachability, None)
        }
        other => {
            unreachable!("dispatch_enrich called with non-enrich command: {other:?}")
        }
    };
    enrich(db, keyspace, verb, workspace)
}

#[cfg(feature = "classify")]
pub(crate) fn dispatch_classify(cmd: Command) -> Result<(), CfdbCliError> {
    match cmd {
        Command::Scope {
            db,
            context,
            workspace,
            format,
            output,
            keyspace,
            explain,
            production_only,
        } => scope(
            &db,
            &context,
            workspace.as_deref(),
            &format,
            output.as_deref(),
            keyspace.as_deref(),
            explain,
            production_only,
        ),
        Command::Classify {
            db,
            keyspace,
            context,
            restrict_to_diff,
            workspace,
            output,
            format,
        } => classify(
            db,
            keyspace,
            context,
            restrict_to_diff,
            output,
            workspace,
            format,
        ),
        Command::Check {
            db,
            keyspace,
            trigger,
            no_fail,
        } => {
            let rows_found = check(&db, &keyspace, trigger)?;
            if rows_found > 0 && !no_fail {
                findings_exit();
            }
            Ok(())
        }
        Command::FindCanonical {
            db,
            keyspace,
            concept,
        } => typed_stub("find_canonical", &db, &keyspace, &[("concept", &concept)]),
        Command::ListBypasses {
            db,
            keyspace,
            concept,
        } => typed_stub("list_bypasses", &db, &keyspace, &[("concept", &concept)]),
        other => unreachable!("dispatch_classify called with non-classify command: {other:?}"),
    }
}
