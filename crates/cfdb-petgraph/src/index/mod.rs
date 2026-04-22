//! Persistent inverted indexes on `:Item` props and computed keys
//! (RFC-035). Lookup + evaluator integration land in slices 5 (#184) and
//! 6 (#185); composition-root wiring in slice 7 (#186).
//!
//! - `spec` — `IndexSpec`, `IndexEntry`, `ComputedKey`, TOML loader
//!   (slice 1 #180).
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
//! Stable Abstractions Principle if placed in the most-depended-on crate
//! (RFC-035 R1 B1 resolution — clean-arch + solid-architect convergent
//! concern).

pub(crate) mod build;
pub(crate) mod posting;
pub mod spec;

pub use spec::{ComputedKey, IndexEntry, IndexSpec, IndexSpecLoadError};
