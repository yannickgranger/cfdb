//! `check-prelude-triggers` — Tier-1 mechanical C-trigger binary per RFC-034 v3.3 §4.2.
//!
//! Fires 5 deterministic triggers against a workspace diff:
//!
//! | Trigger | Concept |
//! |---|---|
//! | C1 | cross-context change — diff touches ≥2 bounded contexts per `context-map.toml` |
//! | C3 | port trait signature — diff touches a file under `crates/ports*/src/` |
//! | C7 | financial-precision path — diff touches a crate listed in `financial-precision-crates.toml` |
//! | C8 | pipeline-stage cross — diff touches ≥2 stages per `pipeline-stages.toml` |
//! | C9 | workspace cardinality — workspace `Cargo.toml` is in the diff |
//!
//! The binary is stateless: it reads argv-supplied paths and emits a versioned
//! JSON envelope on stdout. See [`report::PreludeTriggerReport`] for the shape.
//!
//! Homonym note: the `C*` IDs here are RFC-034 mechanical pre-council triggers
//! and are distinct from cfdb's internal Cypher-query triggers `T1`/`T3`
//! (cfdb issues #100/#101). This binary does NOT implement T-triggers.

pub mod report;
pub mod toml_io;
pub mod trigger_id;
pub mod triggers;

pub use report::PreludeTriggerReport;
pub use toml_io::LoadError;
pub use trigger_id::TriggerId;
