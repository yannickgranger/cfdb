#[derive(Debug, PartialEq, Eq)]
enum NormalizedEntries {
    Strs(std::collections::BTreeSet<String>),
    Ints(std::collections::BTreeSet<i64>),
    Empty,
    MixedOrInvalid,
}

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

pub(super) fn entries_subset_impl(a_json: &str, b_json: &str) -> bool {
    let a = parse_entries_normalized(a_json);
    let b = parse_entries_normalized(b_json);
    match (a, b) {
        (NormalizedEntries::Empty, _) => true,
        (NormalizedEntries::MixedOrInvalid, _) | (_, NormalizedEntries::MixedOrInvalid) => false,
        (NormalizedEntries::Strs(sa), NormalizedEntries::Strs(sb)) => sa.is_subset(&sb),
        (NormalizedEntries::Ints(ia), NormalizedEntries::Ints(ib)) => ia.is_subset(&ib),
        (NormalizedEntries::Strs(_), NormalizedEntries::Ints(_))
        | (NormalizedEntries::Ints(_), NormalizedEntries::Strs(_))
        | (NormalizedEntries::Strs(_), NormalizedEntries::Empty)
        | (NormalizedEntries::Ints(_), NormalizedEntries::Empty) => false,
    }
}

pub(super) fn entries_jaccard_impl(a_json: &str, b_json: &str) -> f64 {
    let a = parse_entries_normalized(a_json);
    let b = parse_entries_normalized(b_json);
    match (a, b) {
        (NormalizedEntries::Empty, NormalizedEntries::Empty) => 0.0,
        (NormalizedEntries::MixedOrInvalid, _) | (_, NormalizedEntries::MixedOrInvalid) => 0.0,
        (NormalizedEntries::Strs(sa), NormalizedEntries::Strs(sb)) => jaccard_btree(&sa, &sb),
        (NormalizedEntries::Ints(ia), NormalizedEntries::Ints(ib)) => jaccard_btree(&ia, &ib),
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

pub(super) fn signatures_differ_modulo_whitespace(a: &str, b: &str) -> bool {
    let mut ai = a.trim().chars().peekable();
    let mut bi = b.trim().chars().peekable();
    loop {
        let (a_ws, a_next) = skip_ws_run(&mut ai);
        let (b_ws, b_next) = skip_ws_run(&mut bi);
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
