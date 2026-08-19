use serde::{Deserialize, Serialize};

use super::labels::{EdgeLabel, Label};
use super::version::SchemaVersion;

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Provenance {
    Extractor,
    EnrichRfcDocs,
    EnrichMetrics,
    EnrichGitHistory,
    EnrichConcepts,
    EnrichReachability,
    Reserved,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AttributeDescriptor {
    pub name: String,
    pub type_hint: String,
    pub description: String,
    pub provenance: Provenance,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NodeLabelDescriptor {
    pub label: Label,
    pub description: String,
    pub attributes: Vec<AttributeDescriptor>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EdgeLabelDescriptor {
    pub label: EdgeLabel,
    pub description: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attributes: Vec<AttributeDescriptor>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub from: Vec<Label>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub to: Vec<Label>,
    #[serde(default = "default_edge_provenance")]
    pub provenance: Provenance,
}

fn default_edge_provenance() -> Provenance {
    Provenance::Extractor
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SchemaDescribe {
    pub schema_version: SchemaVersion,
    pub nodes: Vec<NodeLabelDescriptor>,
    pub edges: Vec<EdgeLabelDescriptor>,
}

pub(super) fn attr(
    name: &str,
    type_hint: &str,
    description: &str,
    provenance: Provenance,
) -> AttributeDescriptor {
    AttributeDescriptor {
        name: name.to_string(),
        type_hint: type_hint.to_string(),
        description: description.to_string(),
        provenance,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provenance_round_trips_as_snake_case() {
        for p in [
            Provenance::Extractor,
            Provenance::EnrichRfcDocs,
            Provenance::EnrichMetrics,
            Provenance::EnrichGitHistory,
            Provenance::EnrichConcepts,
            Provenance::EnrichReachability,
            Provenance::Reserved,
        ] {
            let json = serde_json::to_string(&p).expect("Provenance is a plain derived enum");
            let back: Provenance =
                serde_json::from_str(&json).expect("round-trip of just-serialized Provenance");
            assert_eq!(p, back);
        }
        assert_eq!(
            serde_json::to_string(&Provenance::EnrichRfcDocs)
                .expect("Provenance is a plain derived enum"),
            "\"enrich_rfc_docs\""
        );
        assert_eq!(
            serde_json::to_string(&Provenance::EnrichGitHistory)
                .expect("Provenance is a plain derived enum"),
            "\"enrich_git_history\""
        );
        assert_eq!(
            serde_json::to_string(&Provenance::EnrichReachability)
                .expect("Provenance is a plain derived enum"),
            "\"enrich_reachability\""
        );
        assert_eq!(
            serde_json::to_string(&Provenance::Reserved)
                .expect("Provenance is a plain derived enum"),
            "\"reserved\""
        );
    }
}
