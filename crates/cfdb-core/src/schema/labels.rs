use std::fmt;

use serde::{Deserialize, Serialize};

pub const RECEIVER_POSITION: u32 = 0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ArgKind {
    Path,
    MethodCall,
    Call,
    Ref,
    Literal,
    Other,
}

impl ArgKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ArgKind::Path => "path",
            ArgKind::MethodCall => "method_call",
            ArgKind::Call => "call",
            ArgKind::Ref => "ref",
            ArgKind::Literal => "literal",
            ArgKind::Other => "other",
        }
    }
}

impl fmt::Display for ArgKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

pub const ARG_KIND_PATH: &str = ArgKind::Path.as_str();
pub const ARG_KIND_METHOD_CALL: &str = ArgKind::MethodCall.as_str();
pub const ARG_KIND_CALL: &str = ArgKind::Call.as_str();
pub const ARG_KIND_REF: &str = ArgKind::Ref.as_str();
pub const ARG_KIND_LITERAL: &str = ArgKind::Literal.as_str();
pub const ARG_KIND_OTHER: &str = ArgKind::Other.as_str();

pub const ARG_KINDS: &[&str] = &[
    ARG_KIND_PATH,
    ARG_KIND_METHOD_CALL,
    ARG_KIND_CALL,
    ARG_KIND_REF,
    ARG_KIND_LITERAL,
    ARG_KIND_OTHER,
];

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Label(pub String);

impl Label {
    pub const CRATE: &'static str = "Crate";
    pub const MODULE: &'static str = "Module";
    pub const FILE: &'static str = "File";
    pub const IMPORT: &'static str = "Import";
    pub const ITEM: &'static str = "Item";
    pub const FIELD: &'static str = "Field";
    pub const VARIANT: &'static str = "Variant";
    pub const PARAM: &'static str = "Param";
    pub const CALL_SITE: &'static str = "CallSite";
    pub const ENTRY_POINT: &'static str = "EntryPoint";
    pub const CONCEPT: &'static str = "Concept";
    pub const CONTEXT: &'static str = "Context";
    pub const RFC_DOC: &'static str = "RfcDoc";
    pub const CONST_TABLE: &'static str = "ConstTable";
    pub const LITERAL: &'static str = "Literal";
    pub const ARGUMENT: &'static str = "Argument";
    pub const MATCH_SITE: &'static str = "MatchSite";

    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Label {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for Label {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EdgeLabel(pub String);

impl EdgeLabel {
    pub const IN_CRATE: &'static str = "IN_CRATE";
    pub const IN_MODULE: &'static str = "IN_MODULE";
    pub const HAS_FIELD: &'static str = "HAS_FIELD";
    pub const HAS_IMPORT: &'static str = "HAS_IMPORT";
    pub const HAS_VARIANT: &'static str = "HAS_VARIANT";
    pub const HAS_PARAM: &'static str = "HAS_PARAM";
    pub const HAS_CONST_TABLE: &'static str = "HAS_CONST_TABLE";
    pub const TYPE_OF: &'static str = "TYPE_OF";
    pub const IMPLEMENTS: &'static str = "IMPLEMENTS";
    pub const IMPLEMENTS_FOR: &'static str = "IMPLEMENTS_FOR";
    pub const RETURNS: &'static str = "RETURNS";
    pub const BELONGS_TO: &'static str = "BELONGS_TO";

    pub const CALLS: &'static str = "CALLS";
    pub const INVOKES_AT: &'static str = "INVOKES_AT";

    pub const MATCHES_AT: &'static str = "MATCHES_AT";
    pub const MATCHES_ON: &'static str = "MATCHES_ON";

    pub const EXPOSES: &'static str = "EXPOSES";
    pub const REGISTERS_PARAM: &'static str = "REGISTERS_PARAM";

    pub const LABELED_AS: &'static str = "LABELED_AS";
    pub const CANONICAL_FOR: &'static str = "CANONICAL_FOR";
    pub const EQUIVALENT_TO: &'static str = "EQUIVALENT_TO";

    pub const REFERENCED_BY: &'static str = "REFERENCED_BY";
    pub const HAS_ARG: &'static str = "HAS_ARG";

    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EdgeLabel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for EdgeLabel {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Keyspace(pub String);

impl Keyspace {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Keyspace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Direction {
    Out,
    In,
    Undirected,
}

#[cfg(test)]
mod tests;
