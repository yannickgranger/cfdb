//! Canonical crate-name prefix for HIR-derived qnames.
//!
//! The syn extractor and every `:Item` qname key the crate segment of a
//! qname off the cargo **package** name (`package.name`, dashes replaced
//! by underscores — see `cfdb_core::qname` and
//! `cfdb-extractor/src/file_walker.rs`). A HIR [`Crate`]'s
//! [`display_name`](Crate::display_name) is the crate **target** name
//! instead: for a `[[bin]]` target whose name differs from its
//! `[package] name` (e.g. `[package] name = "cfdb-cli"` +
//! `[[bin]] name = "cfdb"`) the two diverge, and every HIR `EXPOSES` /
//! `CALLS` endpoint built from the target name (`item:cfdb::…`) dangles
//! against the package-name `:Item` (`item:cfdb_cli::…`) — the worst
//! class of graph corruption (#517).
//!
//! The package name is carried on the crate's [`origin`](Crate::origin):
//! `CrateOrigin::Local` (workspace members), `CrateOrigin::Library`
//! (non-member deps), and `CrateOrigin::Rustc` all name the owning
//! package. Language crates (`CrateOrigin::Lang` — std/core/…) carry no
//! package name, so those fall back to `display_name`.

use ra_ap_base_db::CrateOrigin;
use ra_ap_hir::db::HirDatabase;
use ra_ap_hir::Crate;

/// The crate segment (index 0 of a qname's module stack) for `krate`,
/// matching the syn extractor's package-name convention with dashes
/// replaced by underscores. Shared by the entry-point and call-site
/// emitters so a bin-target crate whose name differs from its package
/// produces one qname prefix across both — the cross-producer alignment
/// `cfdb_core::qname` documents as the stable :Item ID contract.
pub(crate) fn crate_qname_prefix<DB>(db: &DB, krate: Crate) -> String
where
    DB: HirDatabase + Sized,
{
    let package_name = match krate.origin(db) {
        CrateOrigin::Local {
            name: Some(name), ..
        }
        | CrateOrigin::Library { name, .. }
        | CrateOrigin::Rustc { name } => name.as_str().to_owned(),
        // No package name on origin (an unnamed local root or a language
        // crate): fall back to the target display name.
        CrateOrigin::Local { name: None, .. } | CrateOrigin::Lang(_) => krate
            .display_name(db)
            .map(|n| n.to_string())
            .unwrap_or_default(),
    };
    package_name.replace('-', "_")
}
