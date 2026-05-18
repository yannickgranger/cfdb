//! Public explain-trace types for RFC-035 slice 7 (#186).
//!
//! Emitted by [`crate::PetgraphStore::execute_explained`] so the `cfdb
//! scope --explain` CLI surface can report which MATCH patterns were
//! satisfied via the `by_prop` inverted index (slice 5 / slice 6 fast
//! paths) and which fell back to a full label scan.
//!
//! **Stability contract (for dogfood tests).** One [`ExplainRow`] per
//! `candidate_nodes` invocation. For multi-MATCH queries the row count
//! per pattern depends on whether the pattern's candidate set is
//! provably binding-independent (#409 fast path in
//! `eval::pattern::apply_node_pattern`):
//!
//! - **Binding-independent** (no foreign-var coupling in props or
//!   WHERE) — `candidate_nodes` fires ONCE per query, regardless of
//!   how many incoming binding rows the pattern sees. Cartesian
//!   classifiers (`MATCH (a:Item), (b:Item)` with no cross-binding
//!   WHERE) hit this path: explain trace shows `(b:Item)` exactly
//!   once, not once per outer `a` row.
//! - **Binding-dependent** (WHERE has a leaf predicate referencing
//!   both `own_var` and a foreign var, e.g. `a.x = b.x`) —
//!   `candidate_nodes` fires once per incoming binding row, as in the
//!   pre-#409 behaviour.
//!
//! Aggregate via
//! `.iter().filter(|r| matches!(r.hit, ExplainHit::Indexed)).count()`
//! for the "indexed MATCH count" metric in the PR body. Dogfood tests
//! that grep for per-row trace counts need to scope assertions to the
//! pattern shape they cover.
//!
//! **Not serialized.** This channel is side-band from `QueryResult` so
//! no explain rows leak into the canonical dump or the keyspace wire
//! format (RFC-035 §4 determinism invariant). The channel lives entirely
//! inside `cfdb-petgraph`.

/// How a single `candidate_nodes` call was satisfied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExplainHit {
    /// `candidates_from_index` fired — the fast path consumed at least
    /// one posting list (label+prop Eq, label+prop literal, or
    /// cross-MATCH bucket intersection).
    Indexed,
    /// `candidates_from_index` returned `None` — the evaluator fell
    /// back to `nodes_with_label` (or `all_nodes_sorted` for
    /// label-less patterns).
    Fallback,
}

/// A single explain trace row. The `pattern` string is a short human-
/// readable summary of the `NodePattern` that was resolved, e.g.
/// `"(a:Item)"` or `"(b:Item)"`. Stable format — dogfood tests grep
/// on the arrow (`→ indexed` / `→ fallback`) and the bracket shape.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExplainRow {
    pub pattern: String,
    pub hit: ExplainHit,
}

impl ExplainRow {
    /// Canonical stable-format one-line rendering used by the
    /// `cfdb scope --explain` CLI path. Format:
    ///
    /// ```text
    /// explain: (b:Item) → indexed
    /// explain: (a:Item) → fallback
    /// ```
    pub fn format_line(&self) -> String {
        let arrow = match self.hit {
            ExplainHit::Indexed => "indexed",
            ExplainHit::Fallback => "fallback",
        };
        format!("explain: {} → {arrow}", self.pattern)
    }
}
