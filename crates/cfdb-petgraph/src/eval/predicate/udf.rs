//! Helpers shared by the `signature_divergent` / `entries_subset` /
//! `entries_jaccard` / `overlap_verdict` Cypher UDFs evaluated in
//! [`super::Evaluator`]. The pure-function impls live here so the
//! enclosing `predicate.rs` stays focused on the visitor + dispatch
//! surface; their unit tests live alongside them (rather than reaching
//! across the file split) so a future change to a helper and its test
//! is a one-file diff.

/// Parsed JSON-array element set for the RFC-040 §3.4 overlap UDFs.
///
/// `entries_normalized` is JSON-array-as-string of either all strings
/// (`["a","b"]`) or all numbers (`[1,2]`) — the element type is
/// inferred from the first parseable element. Mixed-element-type
/// inputs (e.g. `["a", 1]`) are forbidden by the wire contract; the
/// UDFs treat them as `MixedOrInvalid` so the enclosing rule sees no
/// overlap (RFC-040 §3.4 N2).
#[derive(Debug, PartialEq, Eq)]
enum NormalizedEntries {
    Strs(std::collections::BTreeSet<String>),
    Ints(std::collections::BTreeSet<i64>),
    /// Empty input (`[]`) — distinct from MixedOrInvalid because empty
    /// is a valid set (subset of anything; jaccard 0/0 → 0.0). The
    /// element type is unknown but operations against another empty
    /// or any populated set are well-defined.
    Empty,
    /// Either a parse error or a mixed-element-type input. Both UDFs
    /// treat this as "no overlap" rather than propagating a parse
    /// failure, because the wire contract guarantees well-formed
    /// `entries_normalized`; a malformed value is best surfaced as
    /// "no row matches" in the rule rather than an evaluator panic.
    MixedOrInvalid,
}

/// Parse the `entries_normalized` JSON-array string into a sorted set
/// suitable for set-relationship comparison. Element type is inferred
/// from the first element; mixed-type or invalid input collapses to
/// [`NormalizedEntries::MixedOrInvalid`].
fn parse_entries_normalized(s: &str) -> NormalizedEntries {
    let parsed: serde_json::Value = match serde_json::from_str(s) {
        Ok(v) => v,
        Err(_) => return NormalizedEntries::MixedOrInvalid,
    };
    let serde_json::Value::Array(items) = parsed else {
        return NormalizedEntries::MixedOrInvalid;
    };
    if items.is_empty() {
        return NormalizedEntries::Empty;
    }
    // Infer element type from the first element. Number elements are
    // matched as i64 because the extractor emits decimal-stringified
    // integers per RFC-040 §3.4; non-integral floats are not in the
    // wire-shape vocabulary and collapse to MixedOrInvalid.
    //
    // Decide kind via a snapshot of the first element (borrow ends at
    // the matches!), then consume `items` by-value in each branch — the
    // owned String / Number variants move into the result set with no
    // clone, dropping the loop-clone flagged by quality-metrics.
    let first_is_string = matches!(&items[0], serde_json::Value::String(_));
    let first_is_number = matches!(&items[0], serde_json::Value::Number(_));
    if first_is_string {
        let mut set = std::collections::BTreeSet::new();
        for v in items {
            let serde_json::Value::String(s) = v else {
                return NormalizedEntries::MixedOrInvalid;
            };
            set.insert(s);
        }
        NormalizedEntries::Strs(set)
    } else if first_is_number {
        let mut set = std::collections::BTreeSet::new();
        for v in items {
            let serde_json::Value::Number(n) = v else {
                return NormalizedEntries::MixedOrInvalid;
            };
            let Some(i) = n.as_i64() else {
                return NormalizedEntries::MixedOrInvalid;
            };
            set.insert(i);
        }
        NormalizedEntries::Ints(set)
    } else {
        NormalizedEntries::MixedOrInvalid
    }
}

/// `entries_subset` impl — true iff every element of `a` is in `b`.
/// Empty is a subset of anything; equal sets are subsets of each
/// other. Cross-element-type or invalid inputs return `false`.
pub(super) fn entries_subset_impl(a_json: &str, b_json: &str) -> bool {
    let a = parse_entries_normalized(a_json);
    let b = parse_entries_normalized(b_json);
    match (a, b) {
        (NormalizedEntries::Empty, _) => true,
        (NormalizedEntries::MixedOrInvalid, _) | (_, NormalizedEntries::MixedOrInvalid) => false,
        (NormalizedEntries::Strs(sa), NormalizedEntries::Strs(sb)) => sa.is_subset(&sb),
        (NormalizedEntries::Ints(ia), NormalizedEntries::Ints(ib)) => ia.is_subset(&ib),
        // Cross-element-type — no overlap by RFC-040 §3.4 N2. The
        // empty-on-the-right case (e.g. Strs vs Empty) is `false`
        // because a populated set is never a subset of empty; the
        // empty-on-the-left case is handled by the first arm above.
        (NormalizedEntries::Strs(_), NormalizedEntries::Ints(_))
        | (NormalizedEntries::Ints(_), NormalizedEntries::Strs(_))
        | (NormalizedEntries::Strs(_), NormalizedEntries::Empty)
        | (NormalizedEntries::Ints(_), NormalizedEntries::Empty) => false,
    }
}

/// `entries_jaccard` impl — `|a ∩ b| / |a ∪ b|`.
/// Returns `0.0` if both inputs are empty (avoid divide-by-zero) and
/// `0.0` for cross-element-type / invalid input.
pub(super) fn entries_jaccard_impl(a_json: &str, b_json: &str) -> f64 {
    let a = parse_entries_normalized(a_json);
    let b = parse_entries_normalized(b_json);
    match (a, b) {
        (NormalizedEntries::Empty, NormalizedEntries::Empty) => 0.0,
        (NormalizedEntries::MixedOrInvalid, _) | (_, NormalizedEntries::MixedOrInvalid) => 0.0,
        (NormalizedEntries::Strs(sa), NormalizedEntries::Strs(sb)) => jaccard_btree(&sa, &sb),
        (NormalizedEntries::Ints(ia), NormalizedEntries::Ints(ib)) => jaccard_btree(&ia, &ib),
        // Cross-element-type or empty-vs-populated — no overlap. The
        // empty-vs-populated case returns 0.0 because |∩| = 0, |∪| =
        // |populated|, ratio is 0.0.
        (NormalizedEntries::Strs(_), NormalizedEntries::Ints(_))
        | (NormalizedEntries::Ints(_), NormalizedEntries::Strs(_))
        | (NormalizedEntries::Empty, NormalizedEntries::Strs(_))
        | (NormalizedEntries::Empty, NormalizedEntries::Ints(_))
        | (NormalizedEntries::Strs(_), NormalizedEntries::Empty)
        | (NormalizedEntries::Ints(_), NormalizedEntries::Empty) => 0.0,
    }
}

fn jaccard_btree<T: Ord>(
    a: &std::collections::BTreeSet<T>,
    b: &std::collections::BTreeSet<T>,
) -> f64 {
    let inter = a.intersection(b).count() as f64;
    let union = a.union(b).count() as f64;
    if union == 0.0 {
        0.0
    } else {
        inter / union
    }
}

/// `overlap_verdict` impl — RFC-040 §3.4 precedence-decoder.
/// See [`Evaluator::call_overlap_verdict`] for the contract.
pub(super) fn overlap_verdict_impl(
    a_norm: &str,
    b_norm: &str,
    a_hash: &str,
    b_hash: &str,
) -> &'static str {
    if a_hash == b_hash {
        return "CONST_TABLE_DUPLICATE";
    }
    if entries_subset_impl(a_norm, b_norm) || entries_subset_impl(b_norm, a_norm) {
        return "CONST_TABLE_SUBSET";
    }
    if entries_jaccard_impl(a_norm, b_norm) >= 0.5 {
        return "CONST_TABLE_INTERSECTION_HIGH";
    }
    "CONST_TABLE_NONE"
}

// Normalize a signature string for `signature_divergent` comparison —
// `normalize_signature(s)` lives in `udf/tests.rs` (extracted #421
// boy-scout for the god-file split). It trims outer whitespace and
// collapses any run of internal whitespace to a single ASCII space.
// See [`super::Evaluator::call_signature_divergent`] for the rationale.
// Production dispatch goes through [`signatures_differ_modulo_whitespace`]
// (equivalent semantics, zero allocation, #409); the allocating form
// is kept for the unit-test corpus that locks the normalization shape
// AND for the equivalence assertion in `signature_divergent_tests`.

/// `signatures_differ_modulo_whitespace(a, b)` — equivalent to
/// `normalize_signature(a) != normalize_signature(b)` but iterates
/// both inputs once with zero heap allocation. Used by
/// [`super::Evaluator::call_signature_divergent`] on the
/// `signature-divergent.cypher` hot path (#409). Pre-fix wall-time
/// on cfdb-self smoke was 542s for that one query; the double
/// allocation per invocation was the dominant cost.
///
/// # Semantics (locked by `signature_divergent_tests`)
///
/// Two signatures are equivalent (returns `false` = not divergent)
/// iff their non-whitespace characters are identical in order AND
/// the presence/absence of whitespace between each adjacent pair of
/// non-whitespace characters agrees. Outer whitespace is ignored.
///
/// # Algorithm
///
/// The `normalize_signature` form trims outer whitespace and
/// collapses every internal whitespace run to a single space. Two
/// such normalized strings are equal iff:
///
/// 1. Their non-whitespace characters are identical in order, AND
/// 2. Between each adjacent pair of non-whitespace characters, both
///    have either zero whitespace or some non-zero amount.
///
/// We decide both in one fused walk by stepping pairwise through
/// `a.chars()` and `b.chars()` after `trim()`. Track each side's
/// "had-whitespace-since-prev" state via [`skip_ws_run`]; at each
/// non-whitespace step require both the character to match and the
/// had-ws flags to agree.
pub(super) fn signatures_differ_modulo_whitespace(a: &str, b: &str) -> bool {
    let mut ai = a.trim().chars().peekable();
    let mut bi = b.trim().chars().peekable();
    loop {
        let (a_ws, a_next) = skip_ws_run(&mut ai);
        let (b_ws, b_next) = skip_ws_run(&mut bi);
        // Disagreement on whether a whitespace gap appears here =
        // divergent (one has `Foo Bar`, the other has `FooBar`).
        if a_ws != b_ws {
            return true;
        }
        match (a_next, b_next) {
            (None, None) => return false,
            (Some(ca), Some(cb)) if ca == cb => {
                ai.next();
                bi.next();
            }
            _ => return true,
        }
    }
}

/// Advance `iter` past a run of whitespace characters; return whether
/// any whitespace was consumed and a peek at the next non-whitespace
/// character (or `None` at end of input). Helper for
/// [`signatures_differ_modulo_whitespace`].
fn skip_ws_run<I: Iterator<Item = char>>(
    iter: &mut std::iter::Peekable<I>,
) -> (bool, Option<char>) {
    let mut saw_ws = false;
    while let Some(&c) = iter.peek() {
        if c.is_whitespace() {
            iter.next();
            saw_ws = true;
        } else {
            return (saw_ws, Some(c));
        }
    }
    (saw_ws, None)
}

#[cfg(test)]
mod tests;
