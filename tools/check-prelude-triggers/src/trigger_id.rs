use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub enum TriggerId {
    #[serde(rename = "C1")]
    C1,
    #[serde(rename = "C3")]
    C3,
    #[serde(rename = "C7")]
    C7,
    #[serde(rename = "C8")]
    C8,
    #[serde(rename = "C9")]
    C9,
}

impl TriggerId {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::C1 => "C1",
            Self::C3 => "C3",
            Self::C7 => "C7",
            Self::C8 => "C8",
            Self::C9 => "C9",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TriggerId;

    #[test]
    fn serializes_to_rfc034_uppercase_strings() {
        for (id, expected) in [
            (TriggerId::C1, "\"C1\""),
            (TriggerId::C3, "\"C3\""),
            (TriggerId::C7, "\"C7\""),
            (TriggerId::C8, "\"C8\""),
            (TriggerId::C9, "\"C9\""),
        ] {
            let json = serde_json::to_string(&id).expect("serialize");
            assert_eq!(json, expected, "trigger {id:?} serialization");
            assert_eq!(id.as_str(), &expected[1..expected.len() - 1]);
        }
    }
}
