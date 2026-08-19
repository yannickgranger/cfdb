pub(crate) mod build;
pub(crate) mod lookup;
#[cfg(test)]
mod lookup_cross_match_tests;
#[cfg(test)]
mod lookup_tests;
pub(crate) mod posting;
pub mod spec;

pub use spec::{ComputedKey, IndexEntry, IndexSpec, IndexSpecLoadError};
