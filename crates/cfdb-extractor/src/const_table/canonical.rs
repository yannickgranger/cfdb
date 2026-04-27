//! Canonicalization helpers for [`crate::const_table`] (RFC-040 §3.1).
//!
//! `entries_hash`, `entries_normalized`, and `entries_sample` are derived
//! from the recognizer's `entries: Vec<EntryValue>` (declaration order). The
//! visitor calls these to build the wire props.
//!
//! Split out of `const_table.rs` (#350) to keep each file under the 500-LOC
//! budget. Public surface is unchanged — every item visible to the rest of
//! the crate is re-exported from [`super`].

use sha2::{Digest, Sha256};

use super::recognize::EntryValue;

/// Sort a copy of `entries` ascending — lexicographic for `Str`, numeric for
/// `Num`. Two consts with the same set but different declaration order
/// produce identical sorted output (the structural-equality key for the
/// overlap detector, RFC §3.4).
pub(crate) fn canonical_sorted_entries(entries: &[EntryValue]) -> Vec<EntryValue> {
    let mut sorted = entries.to_vec();
    // EntryValue derives Ord through its variants — but the recognizer
    // guarantees the slice is homogeneous (one element_type), so we can
    // sort with a variant-aware comparator without ever crossing the
    // Str/Num boundary. The match below makes the homogeneity assumption
    // explicit.
    sorted.sort_by(|a, b| match (a, b) {
        (EntryValue::Str(x), EntryValue::Str(y)) => x.cmp(y),
        (EntryValue::Num(x), EntryValue::Num(y)) => x.cmp(y),
        // Mixed-variant slices are an extractor invariant violation —
        // the recognizer rejects them in `parse_literal`. Falling back to
        // declaration order keeps sort stable; the panic-free path
        // matches the rest of the extractor's resilience model.
        _ => std::cmp::Ordering::Equal,
    });
    sorted
}

/// sha256 hex (lowercase) over the canonical-sorted entry sequence.
///
/// Encoding per RFC §3.1:
/// - `Str` entries: join with `\0` (NUL never appears in a Rust `&str`
///   literal under syn parsing — safe separator that does not require
///   escaping).
/// - `Num` entries: write each in decimal (no leading zeros, no underscores,
///   no thousands separators), join with `\n`.
///
/// Two consts with the same set produce the same hash regardless of
/// declaration order — this is the structural-equality key for the
/// `const-table-overlap.cypher` detector (RFC §3.4).
pub(crate) fn entries_hash_hex(entries: &[EntryValue]) -> String {
    let sorted = canonical_sorted_entries(entries);
    let mut hasher = Sha256::new();
    let bytes = match sorted.first() {
        // Empty entries → hash the empty sequence; sha256("") is a
        // well-defined fixed string. Either separator would produce the
        // same empty input, so the encoding choice is moot here.
        None => Vec::new(),
        Some(EntryValue::Str(_)) => join_str_entries_nul(&sorted),
        Some(EntryValue::Num(_)) => join_num_entries_newline(&sorted),
    };
    hasher.update(&bytes);
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write;
        // hex formatting is infallible into a String — `write!` returns
        // `Ok(())` for strings; the result is dropped intentionally.
        let _ = write!(&mut hex, "{byte:02x}");
    }
    hex
}

fn join_str_entries_nul(sorted: &[EntryValue]) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, e) in sorted.iter().enumerate() {
        if i > 0 {
            out.push(0u8);
        }
        if let EntryValue::Str(s) = e {
            out.extend_from_slice(s.as_bytes());
        }
    }
    out
}

fn join_num_entries_newline(sorted: &[EntryValue]) -> Vec<u8> {
    let mut out = String::new();
    for (i, e) in sorted.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if let EntryValue::Num(n) = e {
            use std::fmt::Write;
            // i128 Display is canonical decimal — no leading zeros, no
            // underscores. Matches RFC §3.1 wire commitment.
            let _ = write!(&mut out, "{n}");
        }
    }
    out.into_bytes()
}

/// JSON array of the canonical-sorted entries — same byte order used by
/// `entries_hash_hex`. Permanent wire commitment per RFC §3.1: producers
/// re-emit byte-identical normalization across builds; consumers may rely
/// on `JSON.parse` returning a flat array of strings (for `Str`) or
/// integers (for `Num`).
pub(crate) fn entries_normalized_json(entries: &[EntryValue]) -> String {
    let sorted = canonical_sorted_entries(entries);
    encode_entries_json(&sorted)
}

/// JSON array of the FIRST 8 entries in DECLARATION order (no sort, no
/// truncation indicator beyond the natural cap). Triage aid only — two
/// consts with the same set but different declaration order produce
/// divergent samples, which is informational, not a correctness signal
/// (RFC §3.1).
pub(crate) fn entries_sample_json(entries: &[EntryValue]) -> String {
    const SAMPLE_CAP: usize = 8;
    let take = entries.len().min(SAMPLE_CAP);
    encode_entries_json(&entries[..take])
}

fn encode_entries_json(entries: &[EntryValue]) -> String {
    // Build a serde_json::Value array so the encoder handles all string
    // escaping (quotes, backslashes, control chars) per the JSON spec.
    let value = serde_json::Value::Array(
        entries
            .iter()
            .map(|e| match e {
                EntryValue::Str(s) => serde_json::Value::String(s.clone()),
                EntryValue::Num(n) => {
                    // i128 does not impl Into<serde_json::Number>; the
                    // recognizer guarantees the value fits in i128, but
                    // serde_json's number type is bounded by i64/u64/f64.
                    // For values that exceed i64::MAX (i.e. u64 entries
                    // above 2^63-1), encode as a string to preserve the
                    // exact decimal — JSON.parse on the consumer side
                    // will handle either; the wire commitment is "decimal
                    // representation", not "numeric JSON token".
                    if let Ok(n64) = i64::try_from(*n) {
                        serde_json::Value::Number(n64.into())
                    } else if *n >= 0 && *n <= u64::MAX as i128 {
                        serde_json::Value::Number((*n as u64).into())
                    } else {
                        serde_json::Value::String(n.to_string())
                    }
                }
            })
            .collect(),
    );
    serde_json::to_string(&value).unwrap_or_else(|_| String::from("[]"))
}
