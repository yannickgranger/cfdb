use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cfdb_core::qname::TargetDiscriminator;
use ra_ap_hir::db::HirDatabase;
use ra_ap_hir::Crate;
use ra_ap_vfs::Vfs;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TargetRootMap {
    map: BTreeMap<PathBuf, TargetDiscriminator>,
}

impl TargetRootMap {
    #[must_use]
    pub fn from_entries(entries: impl IntoIterator<Item = (PathBuf, TargetDiscriminator)>) -> Self {
        Self {
            map: entries.into_iter().collect(),
        }
    }

    #[must_use]
    pub fn discriminator_for_root(&self, root_file: &Path) -> &TargetDiscriminator {
        self.map.get(root_file).unwrap_or(&TargetDiscriminator::Lib)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

pub(crate) struct EmitCtx<'a> {
    pub(crate) vfs: &'a Vfs,
    pub(crate) targets: &'a TargetRootMap,
}

impl EmitCtx<'_> {
    pub(crate) fn discriminator<DB>(&self, db: &DB, krate: Crate) -> TargetDiscriminator
    where
        DB: HirDatabase + Sized,
    {
        krate_discriminator(db, self.vfs, self.targets, krate)
    }
}

pub(crate) fn krate_discriminator<DB>(
    db: &DB,
    vfs: &Vfs,
    targets: &TargetRootMap,
    krate: Crate,
) -> TargetDiscriminator
where
    DB: HirDatabase + Sized,
{
    let root = krate.root_file(db);
    let vfs_path = vfs.file_path(root);
    match vfs_path.as_path() {
        Some(abs) => targets
            .discriminator_for_root(Path::new(abs.as_str()))
            .clone(),
        None => TargetDiscriminator::Lib,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bin(name: &str) -> TargetDiscriminator {
        TargetDiscriminator::Bin {
            name: name.to_string(),
        }
    }

    #[test]
    fn bin_root_resolves_to_bin_discriminator() {
        let map = TargetRootMap::from_entries([
            (PathBuf::from("/ws/a/src/lib.rs"), TargetDiscriminator::Lib),
            (PathBuf::from("/ws/a/src/main.rs"), bin("tool")),
        ]);
        assert_eq!(
            map.discriminator_for_root(Path::new("/ws/a/src/main.rs")),
            &bin("tool")
        );
        assert_eq!(
            map.discriminator_for_root(Path::new("/ws/a/src/lib.rs")),
            &TargetDiscriminator::Lib
        );
    }

    #[test]
    fn same_named_bin_and_lib_separate_by_root_file() {
        let map = TargetRootMap::from_entries([
            (
                PathBuf::from("/ws/samename/src/lib.rs"),
                TargetDiscriminator::Lib,
            ),
            (PathBuf::from("/ws/samename/src/main.rs"), bin("samename")),
        ]);
        assert_ne!(
            map.discriminator_for_root(Path::new("/ws/samename/src/lib.rs")),
            map.discriminator_for_root(Path::new("/ws/samename/src/main.rs")),
            "lib and same-named bin must resolve to distinct discriminators"
        );
    }

    #[test]
    fn unrecorded_root_falls_back_to_lib() {
        let map = TargetRootMap::from_entries([(PathBuf::from("/ws/a/src/main.rs"), bin("a"))]);
        assert_eq!(
            map.discriminator_for_root(Path::new("/deps/serde/src/lib.rs")),
            &TargetDiscriminator::Lib
        );
        assert_eq!(
            map.discriminator_for_root(Path::new("/ws/a/tests/it.rs")),
            &TargetDiscriminator::Lib
        );
    }

    #[test]
    fn empty_map_is_all_lib() {
        let map = TargetRootMap::default();
        assert!(map.is_empty());
        assert_eq!(
            map.discriminator_for_root(Path::new("/anything.rs")),
            &TargetDiscriminator::Lib
        );
    }
}
