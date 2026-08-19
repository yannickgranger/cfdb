use sha2::{Digest, Sha256};

use super::recognize::EntryValue;

pub(crate) fn canonical_sorted_entries(entries: &[EntryValue]) -> Vec<EntryValue> {
    let mut sorted = entries.to_vec();
    sorted.sort_by(|a, b| match (a, b) {
        (EntryValue::Str(x), EntryValue::Str(y)) => x.cmp(y),
        (EntryValue::Num(x), EntryValue::Num(y)) => x.cmp(y),
        _ => std::cmp::Ordering::Equal,
    });
    sorted
}

pub(crate) fn entries_hash_hex(entries: &[EntryValue]) -> String {
    let sorted = canonical_sorted_entries(entries);
    let mut hasher = Sha256::new();
    let bytes = match sorted.first() {
        None => Vec::new(),
        Some(EntryValue::Str(_)) => join_str_entries_nul(&sorted),
        Some(EntryValue::Num(_)) => join_num_entries_newline(&sorted),
    };
    hasher.update(&bytes);
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write;
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
            let _ = write!(&mut out, "{n}");
        }
    }
    out.into_bytes()
}

pub(crate) fn entries_normalized_json(entries: &[EntryValue]) -> String {
    let sorted = canonical_sorted_entries(entries);
    encode_entries_json(&sorted)
}

pub(crate) fn entries_sample_json(entries: &[EntryValue]) -> String {
    const SAMPLE_CAP: usize = 8;
    let take = entries.len().min(SAMPLE_CAP);
    encode_entries_json(&entries[..take])
}

fn encode_entries_json(entries: &[EntryValue]) -> String {
    let value = serde_json::Value::Array(
        entries
            .iter()
            .map(|e| match e {
                EntryValue::Str(s) => serde_json::Value::String(s.clone()),
                EntryValue::Num(n) => {
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
