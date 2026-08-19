#![allow(unknown_lints)]
#![deny(non_exhaustive_omitted_patterns)]

use ra_ap_base_db as _;
use ra_ap_hir as _;
use ra_ap_hir_def as _;
use ra_ap_hir_expand as _;
use ra_ap_hir_ty as _;
use ra_ap_ide_db as _;
use ra_ap_proc_macro_api as _;
use ra_ap_project_model as _;
use ra_ap_rustc_type_ir as _;
use ra_ap_syntax as _;
use ra_ap_vfs as _;

pub mod emit;

pub mod call_site_emitter;
pub mod error;
pub mod hir_db;

mod crate_name;

pub mod entry_point_emitter;

pub mod target_map;

pub use call_site_emitter::extract_call_sites;
pub use entry_point_emitter::extract_entry_points;
pub use error::HirError;
pub use hir_db::build_hir_database;
pub use ra_ap_proc_macro_api::ProcMacroClient;
pub use target_map::TargetRootMap;
