use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SchemaVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

impl SchemaVersion {
    pub const V0_1_0: Self = Self {
        major: 0,
        minor: 1,
        patch: 0,
    };

    pub const V0_1_1: Self = Self {
        major: 0,
        minor: 1,
        patch: 1,
    };

    pub const V0_1_2: Self = Self {
        major: 0,
        minor: 1,
        patch: 2,
    };

    pub const V0_1_3: Self = Self {
        major: 0,
        minor: 1,
        patch: 3,
    };

    pub const V0_1_4: Self = Self {
        major: 0,
        minor: 1,
        patch: 4,
    };

    pub const V0_2_0: Self = Self {
        major: 0,
        minor: 2,
        patch: 0,
    };

    pub const V0_2_1: Self = Self {
        major: 0,
        minor: 2,
        patch: 1,
    };

    pub const V0_2_2: Self = Self {
        major: 0,
        minor: 2,
        patch: 2,
    };

    pub const V0_2_3: Self = Self {
        major: 0,
        minor: 2,
        patch: 3,
    };

    pub const V0_3_0: Self = Self {
        major: 0,
        minor: 3,
        patch: 0,
    };

    pub const V0_3_1: Self = Self {
        major: 0,
        minor: 3,
        patch: 1,
    };

    pub const V0_3_2: Self = Self {
        major: 0,
        minor: 3,
        patch: 2,
    };

    pub const V0_4_0: Self = Self {
        major: 0,
        minor: 4,
        patch: 0,
    };

    pub const V0_5_0: Self = Self {
        major: 0,
        minor: 5,
        patch: 0,
    };

    pub const V0_6_0: Self = Self {
        major: 0,
        minor: 6,
        patch: 0,
    };

    pub const V0_7_0: Self = Self {
        major: 0,
        minor: 7,
        patch: 0,
    };

    pub const V0_8_0: Self = Self {
        major: 0,
        minor: 8,
        patch: 0,
    };

    pub const CURRENT: Self = Self::V0_8_0;

    pub fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    pub fn can_read(&self, graph_version: &Self) -> bool {
        self.major == graph_version.major && graph_version <= self
    }
}

impl fmt::Display for SchemaVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}
