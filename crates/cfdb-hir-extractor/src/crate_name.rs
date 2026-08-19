use ra_ap_base_db::CrateOrigin;
use ra_ap_hir::db::HirDatabase;
use ra_ap_hir::Crate;

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
        CrateOrigin::Local { name: None, .. } | CrateOrigin::Lang(_) => krate
            .display_name(db)
            .map(|n| n.to_string())
            .unwrap_or_default(),
    };
    package_name.replace('-', "_")
}
