use std::collections::BTreeMap;

use sha2::{Digest, Sha256};

use super::FnItem;

pub(crate) fn compute_dup_cluster_ids(items: &[FnItem]) -> BTreeMap<String, String> {
    let by_sig: BTreeMap<String, Vec<String>> = items
        .iter()
        .filter_map(|item| {
            let sig = item.signature_hash.as_deref()?;
            Some((sig.to_string(), item.qname.clone()))
        })
        .fold(BTreeMap::new(), |mut acc, (sig, qname)| {
            acc.entry(sig).or_insert_with(Vec::new).push(qname);
            acc
        });

    by_sig
        .into_values()
        .filter(|members| members.len() >= 2)
        .flat_map(|members| {
            let cluster_id = hash_cluster(&members);
            members
                .into_iter()
                .map(move |qname| (qname, cluster_id.clone()))
        })
        .collect()
}

pub(crate) fn hash_cluster(members_unsorted: &[String]) -> String {
    let mut sorted: Vec<&str> = members_unsorted.iter().map(String::as_str).collect();
    sorted.sort();
    let joined = sorted.join("\n");
    let digest = Sha256::digest(joined.as_bytes());
    hex_encode(&digest)
}

fn hex_encode(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0F) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fn_item(qname: &str, sig: Option<&str>) -> FnItem {
        FnItem {
            qname: qname.into(),
            name: qname.rsplit("::").next().unwrap_or(qname).into(),
            file: "x.rs".into(),
            signature_hash: sig.map(str::to_string),
            id: format!("item:{qname}"),
        }
    }

    #[test]
    fn same_signature_hash_emits_matching_cluster_id() {
        let items = vec![
            fn_item("crate::a::foo", Some("sig-A")),
            fn_item("crate::b::foo", Some("sig-A")),
        ];
        let out = compute_dup_cluster_ids(&items);
        assert_eq!(out.len(), 2);
        assert_eq!(out["crate::a::foo"], out["crate::b::foo"]);
    }

    #[test]
    fn singleton_receives_no_cluster_id() {
        let items = vec![fn_item("crate::a::foo", Some("sig-A"))];
        let out = compute_dup_cluster_ids(&items);
        assert!(out.is_empty());
    }

    #[test]
    fn missing_signature_hash_excluded() {
        let items = vec![
            fn_item("crate::a::foo", None),
            fn_item("crate::b::foo", None),
        ];
        let out = compute_dup_cluster_ids(&items);
        assert!(out.is_empty());
    }

    #[test]
    fn cluster_id_is_stable_regardless_of_input_order() {
        let a = compute_dup_cluster_ids(&[
            fn_item("crate::b::foo", Some("sig-X")),
            fn_item("crate::a::foo", Some("sig-X")),
        ]);
        let b = compute_dup_cluster_ids(&[
            fn_item("crate::a::foo", Some("sig-X")),
            fn_item("crate::b::foo", Some("sig-X")),
        ]);
        assert_eq!(a["crate::a::foo"], b["crate::a::foo"]);
    }

    #[test]
    fn cluster_id_matches_expected_sha256_hex() {
        let members = vec!["crate::a::foo".to_string(), "crate::b::foo".to_string()];
        let id = hash_cluster(&members);
        let mut hasher = Sha256::new();
        hasher.update(b"crate::a::foo\ncrate::b::foo");
        let expected = hex_encode(&hasher.finalize());
        assert_eq!(id, expected);
        assert_eq!(id.len(), 64, "sha256 hex must be 64 chars");
    }

    #[test]
    fn three_member_cluster_all_get_same_id() {
        let items = vec![
            fn_item("crate::c::foo", Some("sig-Y")),
            fn_item("crate::a::foo", Some("sig-Y")),
            fn_item("crate::b::foo", Some("sig-Y")),
        ];
        let out = compute_dup_cluster_ids(&items);
        assert_eq!(out.len(), 3);
        let first = &out["crate::a::foo"];
        assert_eq!(&out["crate::b::foo"], first);
        assert_eq!(&out["crate::c::foo"], first);
    }

    #[test]
    fn different_signature_hashes_get_different_ids() {
        let items = vec![
            fn_item("crate::a::foo", Some("sig-1")),
            fn_item("crate::b::foo", Some("sig-1")),
            fn_item("crate::c::bar", Some("sig-2")),
            fn_item("crate::d::bar", Some("sig-2")),
        ];
        let out = compute_dup_cluster_ids(&items);
        assert_ne!(out["crate::a::foo"], out["crate::c::bar"]);
        assert_eq!(out["crate::a::foo"], out["crate::b::foo"]);
        assert_eq!(out["crate::c::bar"], out["crate::d::bar"]);
    }
}
