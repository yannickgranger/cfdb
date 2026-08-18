//! Coupling-analysis and pattern-evaluation helpers extracted from
//! `pattern.rs` to keep the parent under the 500-line god-file threshold.
//! These helpers are private to the `pattern` module — kept `pub(super)` so
//! the `impl Evaluator` block in `pattern.rs` can reach them.

use cfdb_core::graph::{GraphReader, NodeHandle};
use cfdb_core::query::{EdgePattern, NodePattern, Pattern, Predicate};
use cfdb_core::result::RowValue;

use super::super::{Binding, Bindings};

/// Per-row body of [`Evaluator::apply_unwind`] — iterator-chain form so
/// the per-item clones do not register as clones-in-loop (the outer `for`
/// loop body now contains only a helper call).
pub(super) fn unwind_row(
    out: &mut Vec<Bindings>,
    bindings: &Bindings,
    items: &[cfdb_core::fact::PropValue],
    var: &str,
) {
    items.iter().for_each(|item| {
        let mut next = bindings.clone();
        next.insert(
            var.to_string(),
            Binding::Value(RowValue::Scalar(item.clone())),
        );
        out.push(next);
    });
}

/// Static analysis — return `true` iff the candidate set produced by
/// [`Evaluator::candidate_nodes`] is provably the same for every incoming
/// binding row. The cached fast path in [`Evaluator::apply_node_pattern`] is
/// only safe under this predicate.
///
/// The candidate set is independent iff every LEAF predicate inside the
/// top-level WHERE is either:
///
/// 1. **Non-coupling** — references only `own_var` or only foreign vars
///    (never both). Foreign-only predicates filter the cartesian product
///    AFTER candidate emission and don't affect `own_var`'s candidate
///    set; own-only predicates produce the same narrowed set on every
///    row.
/// 2. **Coupling, but provably empty in this keyspace** — a leaf like
///    `a.dup_cluster_id = b.dup_cluster_id` IS narrow-eligible by
///    shape (`lookup::candidates_from_index` slice-6), but if the
///    `(label, prop)` index bucket is missing or empty,
///    the narrow can never fire on any row in this keyspace. The
///    lookup falls back to `nodes_with_label`, which IS
///    binding-independent. Cache is safe.
///
/// **Why this matters empirically**: on cfdb-self the `hsb-cluster.cypher`
/// rule joins on `dup_cluster_id`, a prop only populated when ≥2 fns share
/// a `signature_hash`. cfdb has no such duplicates so the prop is null on
/// every `:Item`. The original (keyspace-blind) check classified the
/// predicate as narrowing-coupling and forced per-row iteration over 25k ×
/// 25k = 625M pairs, taking ~4 minutes to produce zero rows. With the
/// keyspace-aware check the empty bucket triggers the cache fast path,
/// collapsing the cost to a single label scan.
///
/// `Or` / `Not` / `NotExists` are treated conservatively as coupling
/// (no keyspace probe; per-row safety preserved). These shapes are
/// vanishingly rare in cypher-rule files and the audit surface isn't
/// worth widening.
///
/// **Why static, not dynamic.** Probing the bound-var closure to detect
/// dependence at runtime would burn allocation on every row and require
/// a fallback if the second row's binding diverged. The keyspace probe
/// runs once per query dispatch and is exact for the AST shapes the
/// parser produces today.
pub(super) fn is_binding_independent_pattern<G: GraphReader + ?Sized>(
    np: &NodePattern,
    where_clause: Option<&Predicate>,
    state: &G,
) -> bool {
    let own_var: Option<&str> = np.var.as_deref();
    let label = match &np.label {
        Some(l) => l,
        // Anonymous-label pattern has no posting list to probe; fall
        // back to the keyspace-blind check (treat any own/foreign
        // coupling as binding-dependent).
        None => {
            return match where_clause {
                None => true,
                Some(pred) => !predicate_couples_own_to_foreign(pred, own_var),
            };
        }
    };
    match where_clause {
        None => true,
        Some(pred) => !predicate_has_active_coupling(pred, own_var, label, state),
    }
}

/// `true` iff any leaf predicate references both `own_var` AND a foreign
/// var AND the narrow-eligible prop (if any) is actually populated in
/// the keyspace. A coupling leaf whose prop has no populated index bucket
/// can never narrow on any row, so it's safe to treat as cache-eligible.
///
/// Walks `And`/`Or`/`Not` into their leaves so a compound
/// `(a.x = b.x) AND (b.kind = 'fn')` still trips the coupling flag on
/// the first leaf if `(Item, x)` has a populated bucket.
pub(super) fn predicate_has_active_coupling<G: GraphReader + ?Sized>(
    pred: &Predicate,
    own_var: Option<&str>,
    label: &cfdb_core::schema::Label,
    state: &G,
) -> bool {
    match pred {
        Predicate::And(a, b) | Predicate::Or(a, b) => {
            predicate_has_active_coupling(a, own_var, label, state)
                || predicate_has_active_coupling(b, own_var, label, state)
        }
        Predicate::Not(inner) => predicate_has_active_coupling(inner, own_var, label, state),
        Predicate::Compare { left, right, .. }
        | Predicate::In { left, right }
        | Predicate::Ne { left, right } => {
            leaf_couples(left, right, own_var)
                && coupling_prop_is_populated(left, right, own_var, label, state)
        }
        Predicate::Regex { left, pattern } => {
            leaf_couples(left, pattern, own_var)
                && coupling_prop_is_populated(left, pattern, own_var, label, state)
        }
        // Conservative — `NotExists { inner }` carries a sub-Query whose
        // bindings escape this scope. Treat as coupling, per-row path
        // (no false caching).
        Predicate::NotExists { .. } => true,
    }
}

/// `true` iff the coupling leaf carries a `Property{own_var, prop}` or
/// `Call{f, [Property{own_var, prop}]}` reference where the relevant
/// `(label, prop)` posting list in `state.by_prop` is populated. If the
/// bucket map for that pair is missing or empty, the narrow can never
/// fire on this keyspace and the cache is safe.
///
/// When `own_var` is anonymous (None), we return `true` (conservative —
/// can't probe an empty bucket without a var to anchor the prop). When
/// the leaf references multiple props (e.g. an `In` with a list of
/// foreign props), we require at least one to be populated.
pub(super) fn coupling_prop_is_populated<G: GraphReader + ?Sized>(
    a: &cfdb_core::query::Expr,
    b: &cfdb_core::query::Expr,
    own_var: Option<&str>,
    label: &cfdb_core::schema::Label,
    state: &G,
) -> bool {
    let Some(own) = own_var else {
        return true;
    };
    let mut tags: Vec<String> = Vec::new();
    collect_narrow_tags(a, own, &mut tags);
    collect_narrow_tags(b, own, &mut tags);
    if tags.is_empty() {
        // Coupling shape with no own-var-anchored prop reference — the
        // narrow can't pick this up either. Treat as not-active so the
        // cache fires.
        return false;
    }
    tags.iter()
        .any(|tag| state.indexed_prop_is_populated(label, tag))
}

/// Extract index tags an `Expr` could feed to `lookup::candidates_from_index`
/// for the `own_var`-bound side. Two shapes match the narrow's
/// recognition surface:
///
/// - `Expr::Property { var: own_var, prop }` → tag = `prop`
/// - `Expr::Call { name, args: [Property{var: own_var, prop}] }` →
///   tag = `"<name>(<prop>)"` (the canonical computed-key form per
///   `.cfdb/indexes.toml`)
pub(super) fn collect_narrow_tags(
    expr: &cfdb_core::query::Expr,
    own_var: &str,
    out: &mut Vec<String>,
) {
    use cfdb_core::query::Expr as E;
    match expr {
        E::Property { var, prop } if var == own_var => {
            out.push(prop.clone());
        }
        E::Call { name, args } if args.len() == 1 => {
            if let E::Property { var, prop } = &args[0] {
                if var == own_var {
                    out.push(format!("{name}({prop})"));
                }
            }
        }
        _ => {}
    }
}

/// `true` iff any leaf predicate references both `own_var` AND a
/// foreign var on either side. Retained for the anonymous-label
/// fallback path in [`is_binding_independent_pattern`].
pub(super) fn predicate_couples_own_to_foreign(pred: &Predicate, own_var: Option<&str>) -> bool {
    match pred {
        Predicate::And(a, b) | Predicate::Or(a, b) => {
            predicate_couples_own_to_foreign(a, own_var)
                || predicate_couples_own_to_foreign(b, own_var)
        }
        Predicate::Not(inner) => predicate_couples_own_to_foreign(inner, own_var),
        Predicate::Compare { left, right, .. }
        | Predicate::In { left, right }
        | Predicate::Ne { left, right } => leaf_couples(left, right, own_var),
        Predicate::Regex { left, pattern } => leaf_couples(left, pattern, own_var),
        Predicate::NotExists { .. } => true,
    }
}

/// Leaf coupling check: gather every var referenced across both sides
/// of the predicate; coupling iff `own_var` appears AND any other var
/// also appears.
pub(super) fn leaf_couples(
    a: &cfdb_core::query::Expr,
    b: &cfdb_core::query::Expr,
    own_var: Option<&str>,
) -> bool {
    let mut vars: Vec<&str> = Vec::new();
    collect_expr_vars_into(a, &mut vars);
    collect_expr_vars_into(b, &mut vars);
    let mentions_own = own_var.is_some_and(|v| vars.contains(&v));
    let mentions_foreign = vars.iter().any(|v| Some(*v) != own_var);
    mentions_own && mentions_foreign
}

/// Walk an [`Expr`] and push every referenced var name (from
/// `Property` or bare `Var`) into `acc`. Literal / Param / List /
/// Call walked recursively.
pub(super) fn collect_expr_vars_into<'e>(expr: &'e cfdb_core::query::Expr, acc: &mut Vec<&'e str>) {
    use cfdb_core::query::Expr as E;
    match expr {
        E::Property { var, .. } | E::Var(var) => acc.push(var.as_str()),
        E::Literal(_) | E::Param(_) => {}
        E::List(items) => {
            for item in items {
                collect_expr_vars_into(item, acc);
            }
        }
        E::Call { args, .. } => {
            for arg in args {
                collect_expr_vars_into(arg, acc);
            }
        }
    }
}

pub(super) fn matches_existing(existing: &Binding, h: NodeHandle) -> bool {
    matches!(existing, Binding::NodeRef(i) if *i == h)
}

pub(super) fn edge_label_matches(pattern: &EdgePattern, edge: &cfdb_core::fact::Edge) -> bool {
    match &pattern.label {
        Some(lbl) => edge.label == *lbl,
        None => true,
    }
}

pub(super) fn collect_pattern_vars(pattern: &Pattern) -> Vec<String> {
    let mut out = Vec::new();
    match pattern {
        Pattern::Node(np) => {
            if let Some(v) = &np.var {
                out.push(v.clone());
            }
        }
        Pattern::Path(pp) => {
            if let Some(v) = &pp.from.var {
                out.push(v.clone());
            }
            if let Some(v) = &pp.to.var {
                out.push(v.clone());
            }
            if let Some(v) = &pp.edge.var {
                out.push(v.clone());
            }
        }
        Pattern::Optional(inner) => out.extend(collect_pattern_vars(inner)),
        Pattern::Unwind { var, .. } => out.push(var.clone()),
    }
    out
}
