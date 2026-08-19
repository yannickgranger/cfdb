#![allow(unknown_lints)]
#![deny(non_exhaustive_omitted_patterns)]

mod engine;
mod eval;
pub mod explain;

pub use engine::QueryEngine;
