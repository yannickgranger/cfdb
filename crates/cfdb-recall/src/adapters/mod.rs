//! Adapters — turn external tool output into `BTreeSet<PublicItem>`.
//!
//! The pure `compute_recall` function takes three sets as input; this
//! module is responsible for producing those sets. Every subprocess call,
//! JSON parse, and syn traversal lives here so that the business-rule
//! tests in `lib.rs` can stay I/O-free.

pub mod extractor;
pub mod ground_truth;
