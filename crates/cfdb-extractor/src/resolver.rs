use std::collections::BTreeMap;

use cfdb_core::fact::Edge;
use cfdb_core::qname::{item_node_id_for_target, TargetDiscriminator};
use cfdb_core::schema::EdgeLabel;

use crate::emitter::Emitter;

fn resolve_to_target_id(
    emitter_qnames: &BTreeMap<String, Vec<TargetDiscriminator>>,
    by_last_segment: &BTreeMap<&str, Option<&String>>,
    type_string: &str,
    src_target: &TargetDiscriminator,
) -> Option<String> {
    let qname = resolve_type_string(emitter_qnames, by_last_segment, type_string)?;
    let chosen = TargetDiscriminator::choose_claim(emitter_qnames.get(&qname)?, src_target)?;
    Some(item_node_id_for_target(&qname, chosen))
}

pub(crate) fn resolve_deferred_returns(emitter: &mut Emitter) {
    let deferred: Vec<(String, TargetDiscriminator, String, syn::Type)> =
        std::mem::take(&mut emitter.deferred_returns);

    let by_last_segment = build_last_segment_index(&emitter.emitted_item_qnames);

    let resolved: Vec<(String, String)> = deferred
        .into_iter()
        .flat_map(|(fn_qname, src_target, return_type, return_ty)| {
            let mut targets: Vec<String> = Vec::new();
            if let Some(dst_id) = resolve_to_target_id(
                &emitter.emitted_item_qnames,
                &by_last_segment,
                &return_type,
                &src_target,
            ) {
                targets.push(dst_id);
            } else {
                targets.extend(
                    crate::type_render::render_type_inner(&return_ty, 3)
                        .into_iter()
                        .filter_map(|candidate| {
                            resolve_to_target_id(
                                &emitter.emitted_item_qnames,
                                &by_last_segment,
                                &candidate,
                                &src_target,
                            )
                        }),
                );
            }
            let src_id = item_node_id_for_target(&fn_qname, &src_target);
            targets
                .into_iter()
                .map(move |dst_id| (src_id.clone(), dst_id))
        })
        .collect();

    for (src_id, dst_id) in resolved {
        emitter.emit_edge(Edge {
            src: src_id,
            dst: dst_id,
            label: EdgeLabel::new(EdgeLabel::RETURNS),
            props: BTreeMap::new(),
        });
    }
}

pub(crate) fn resolve_deferred_type_of(emitter: &mut Emitter) {
    let deferred: Vec<(String, String, &'static str, syn::Type, TargetDiscriminator)> =
        std::mem::take(&mut emitter.deferred_type_of);

    let by_last_segment = build_last_segment_index(&emitter.emitted_item_qnames);

    let resolved: Vec<(String, String)> = deferred
        .into_iter()
        .flat_map(|(src_id, type_string, _label, src_ty, src_target)| {
            let mut targets: Vec<String> = Vec::new();
            if let Some(dst_id) = resolve_to_target_id(
                &emitter.emitted_item_qnames,
                &by_last_segment,
                &type_string,
                &src_target,
            ) {
                targets.push(dst_id);
            } else {
                targets.extend(
                    crate::type_render::render_type_inner(&src_ty, 3)
                        .into_iter()
                        .filter_map(|candidate| {
                            resolve_to_target_id(
                                &emitter.emitted_item_qnames,
                                &by_last_segment,
                                &candidate,
                                &src_target,
                            )
                        }),
                );
            }
            targets
                .into_iter()
                .map(move |dst_id| (src_id.clone(), dst_id))
        })
        .collect();

    for (src_id, dst_id) in resolved {
        emitter.emit_edge(Edge {
            src: src_id,
            dst: dst_id,
            label: EdgeLabel::new(EdgeLabel::TYPE_OF),
            props: BTreeMap::new(),
        });
    }
}

pub(crate) fn resolve_deferred_match_targets(emitter: &mut Emitter) {
    let deferred: Vec<(String, String, TargetDiscriminator)> =
        std::mem::take(&mut emitter.deferred_match_targets);

    let by_last_segment = build_last_segment_index(&emitter.emitted_item_qnames);

    let resolved: Vec<(String, String)> = deferred
        .into_iter()
        .filter_map(|(site_id, matched_path, src_target)| {
            resolve_type_string(
                &emitter.emitted_item_qnames,
                &by_last_segment,
                &matched_path,
            )
            .filter(|q| is_segment_suffix(&matched_path, q))
            .and_then(|target_qname| {
                let claims = emitter.emitted_enum_qnames.get(&target_qname)?;
                let chosen = TargetDiscriminator::choose_claim(claims, &src_target)?;
                Some((site_id, item_node_id_for_target(&target_qname, chosen)))
            })
        })
        .collect();

    for (site_id, dst_id) in resolved {
        emitter.emit_edge(Edge {
            src: site_id,
            dst: dst_id,
            label: EdgeLabel::new(EdgeLabel::MATCHES_ON),
            props: BTreeMap::new(),
        });
    }
}

fn build_last_segment_index(
    emitted_item_qnames: &BTreeMap<String, Vec<TargetDiscriminator>>,
) -> BTreeMap<&str, Option<&String>> {
    let mut by_last_segment: BTreeMap<&str, Option<&String>> = BTreeMap::new();
    for qname in emitted_item_qnames.keys() {
        let seg = cfdb_core::qname::last_segment(qname);
        by_last_segment
            .entry(seg)
            .and_modify(|v| *v = None)
            .or_insert(Some(qname));
    }
    by_last_segment
}

fn resolve_type_string(
    emitted_item_qnames: &BTreeMap<String, Vec<TargetDiscriminator>>,
    by_last_segment: &BTreeMap<&str, Option<&String>>,
    type_string: &str,
) -> Option<String> {
    if emitted_item_qnames.contains_key(type_string) {
        return Some(type_string.to_string());
    }
    let seg = cfdb_core::qname::last_segment(type_string);
    by_last_segment.get(seg).copied().flatten().cloned()
}

fn is_segment_suffix(prefix: &str, qname: &str) -> bool {
    let prefix_segments: Vec<&str> = prefix.split("::").collect();
    let qname_segments: Vec<&str> = qname.split("::").collect();
    qname_segments.ends_with(&prefix_segments)
}

#[cfg(test)]
mod tests;
