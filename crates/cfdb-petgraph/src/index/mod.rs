//! Persistent inverted indexes on `:Item` props and computed keys.
//!
//! - `spec` — `IndexSpec`, `IndexEntry`, `ComputedKey`, TOML loader.
//! - `build` — pure helpers for computing the `(tag, value)` to insert
//!   into `KeyspaceState::by_prop` for a given `(IndexEntry, Node)`
//!   pair (slice 2 #181).
//! - `posting` — `by_prop` posting-list maintenance (insert / remove)
//!   used by the per-node reconcile path. Keeps the per-triple work
//!   off the `reconcile_index_entries` loop body so the metric
//!   scanner sees no `.clone()` calls inside the for-loop.
//!
//! `IndexSpec`, `IndexEntry`, and `ComputedKey` are defined **here** in
//! `cfdb-petgraph`, not in `cfdb-core`: they are backend-optimisation
//! artefacts with no stable abstract meaning and would violate the
//! Stable Abstractions Principle if placed in the most-depended-on crate.

pub(crate) mod build;
pub(crate) mod lookup;
#[cfg(test)]
mod lookup_cross_match_tests;
#[cfg(test)]
mod lookup_tests;
pub(crate) mod posting;
pub mod spec;

pub use spec::{ComputedKey, IndexEntry, IndexSpec, IndexSpecLoadError};
