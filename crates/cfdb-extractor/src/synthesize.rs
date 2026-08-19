use std::collections::BTreeMap;

use cfdb_concepts::{compute_bounded_context, ConceptOverrides};
use cfdb_core::fact::{Node, Props};
use cfdb_core::qname::{display_qname_from_node_id, item_node_id};
use cfdb_core::query::item_kind::ItemKind;
use cfdb_core::schema::{EdgeLabel, Label};

#[cfg(test)]
use cfdb_core::fact::PropValue;

use crate::emitter::Emitter;

pub(crate) fn synthesize_referenced_items(emitter: &mut Emitter, overrides: &ConceptOverrides) {
    let mut synth: BTreeMap<String, &'static str> = BTreeMap::new();
    for edge in emitter.edges() {
        let label = edge.label.as_str();
        let evidence = match label {
            EdgeLabel::IMPLEMENTS => EdgeLabel::IMPLEMENTS,
            EdgeLabel::IMPLEMENTS_FOR => EdgeLabel::IMPLEMENTS_FOR,
            EdgeLabel::RETURNS => EdgeLabel::RETURNS,
            EdgeLabel::TYPE_OF => EdgeLabel::TYPE_OF,
            _ => continue,
        };
        let dst_qname = display_qname_from_node_id(&edge.dst);
        if emitter.emitted_item_qnames.contains_key(dst_qname) {
            continue;
        }
        match synth.get(dst_qname) {
            Some(&existing) if existing == EdgeLabel::IMPLEMENTS => {}
            _ if evidence == EdgeLabel::IMPLEMENTS => {
                synth.insert(dst_qname.to_string(), evidence);
            }
            None => {
                synth.insert(dst_qname.to_string(), evidence);
            }
            Some(_) => {}
        }
    }

    let mut bc_memo: BTreeMap<String, String> = BTreeMap::new();
    for (qname, evidence) in synth {
        let kind = kind_for_evidence(evidence);
        let crate_name = crate_from_qname(&qname);
        let bounded_context = memoized_bounded_context(&mut bc_memo, &crate_name, overrides);
        let props = build_synthetic_item_props(&qname, kind, &crate_name, &bounded_context);

        emitter.emit_node(Node {
            id: item_node_id(&qname),
            label: Label::new(Label::ITEM),
            props,
        });
        emitter.claim_item_qname(&qname, &cfdb_core::qname::TargetDiscriminator::Lib);
    }
}

fn memoized_bounded_context(
    bc_memo: &mut BTreeMap<String, String>,
    crate_name: &str,
    overrides: &ConceptOverrides,
) -> String {
    if let Some(existing) = bc_memo.get(crate_name) {
        return existing.clone();
    }
    let computed = compute_bounded_context(crate_name, overrides).name;
    bc_memo.insert(crate_name.to_string(), computed.clone());
    computed
}

fn build_synthetic_item_props(
    qname: &str,
    kind: &str,
    crate_name: &str,
    bounded_context: &str,
) -> Props {
    cfdb_core::fact::build_item_props(qname, kind, crate_name, bounded_context)
}

fn kind_for_evidence(evidence: &'static str) -> &'static str {
    match evidence {
        EdgeLabel::IMPLEMENTS => ItemKind::Trait.to_extractor_str(),
        _ => ItemKind::Struct.to_extractor_str(),
    }
}

fn crate_from_qname(qname: &str) -> String {
    qname
        .split_once("::")
        .map(|(c, _)| c.to_string())
        .unwrap_or_else(|| qname.to_string())
}

#[cfg(test)]
mod tests;
