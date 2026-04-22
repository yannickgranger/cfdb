//! Persistent inverted indexes on `:Item` props and computed keys
//! (RFC-035, slice 1 #180 — spec + TOML loader only; build pass, lookup,
//! and evaluator integration land in slices 2, 5, 6, 7).
//!
//! `IndexSpec`, `IndexEntry`, and `ComputedKey` are defined **here** in
//! `cfdb-petgraph`, not in `cfdb-core`: they are backend-optimisation
//! artefacts with no stable abstract meaning and would violate the
//! Stable Abstractions Principle if placed in the most-depended-on crate
//! (RFC-035 R1 B1 resolution — clean-arch + solid-architect convergent
//! concern).

pub mod spec;

pub use spec::{ComputedKey, IndexEntry, IndexSpec, IndexSpecLoadError};
