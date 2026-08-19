pub mod count_items;
pub mod extracted_files;
pub mod feature_guard;
pub mod grep_deprecated;
pub mod grep_rfc_docs;
pub mod passes;
pub mod runner;
pub mod scan_concepts;
pub mod thresholds;

pub const EXIT_OK: i32 = 0;

pub const EXIT_VIOLATIONS: i32 = 30;

pub const EXIT_RUNTIME_ERROR: i32 = 1;
