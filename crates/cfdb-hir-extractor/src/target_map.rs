//! RFC-054 54-C (#558): the target-root correlation map.
//!
//! ra_ap exposes NO target kind on `CrateData` / `hir::Crate` — cargo's
//! `TargetData.kind` collapses into `is_proc_macro` during crate-graph
//! construction and is discarded, `CrateOrigin::is_lib()` means
//! "non-member dependency" (a false friend), and for a bin named like
//! its package, `origin` and `display_name` are byte-identical between
//! the lib and bin crate inputs (council rust-systems Finding 1,
//! verified against vendored ra_ap 0.0.328). The ONLY durable join key
//! between a `hir::Crate` and its cargo target is the target's root
//! source file: `hir::Crate::root_file(db)` on one side,
//! `TargetData.root` on the other.
//!
//! [`build_hir_database`](crate::build_hir_database) constructs this map
//! once per load from the retained `CargoWorkspace` (workspace members
//! only) and returns it alongside the database; the emitters correlate
//! each item's crate root through it to derive the RFC-054
//! [`TargetDiscriminator`] its ids are routed through.
//!
//! Path-representation residual: the map records each target root under
//! cargo-metadata's spelling AND its canonicalized form, but ra_ap never
//! resolves symlinks in VFS paths — a member whose VFS spelling matches
//! neither (an in-tree symlink arrangement the top-level-root
//! canonicalization cannot see) silently degrades to Lib for that one
//! crate. Total correlation failure is caught by the join tests
//! (`tests/entry_point_bin_name.rs` asserts `#bin:` presence); the
//! partial single-crate case is accepted residual risk.
//!
//! Only `[lib]` and `[[bin]]` targets are recorded: RFC-054's identity
//! namespace discriminates lib vs bin. Test / example / bench /
//! build-script targets — and any non-member or non-cargo crate (deps,
//! sysroot, language crates) — miss the map and resolve to
//! [`TargetDiscriminator::Lib`], keeping their ids byte-stable with
//! pre-54-C extracts (those crates have no syn-side `:Item`s to join
//! against either way).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use cfdb_core::qname::TargetDiscriminator;
use ra_ap_hir::db::HirDatabase;
use ra_ap_hir::Crate;
use ra_ap_vfs::Vfs;

/// Map from a cargo target's root source file (absolute, as the VFS
/// reports paths) to the [`TargetDiscriminator`] for items of the crate
/// rooted there. Plain `std` + `cfdb-core` value — no `ra_ap` type in
/// the public signature, so the petgraph adapter and `cfdb-cli` stay
/// insulated from the ra-ap bundle (RFC-032 §3 compile-cost boundary).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TargetRootMap {
    map: BTreeMap<PathBuf, TargetDiscriminator>,
}

impl TargetRootMap {
    /// Build from explicit `(target root, discriminator)` entries.
    /// Production population happens in `build_hir_database` from the
    /// retained `CargoWorkspace`; tests construct fixtures directly.
    #[must_use]
    pub fn from_entries(entries: impl IntoIterator<Item = (PathBuf, TargetDiscriminator)>) -> Self {
        Self {
            map: entries.into_iter().collect(),
        }
    }

    /// The discriminator for the crate whose root source file is
    /// `root_file`. A miss is [`TargetDiscriminator::Lib`] — non-member
    /// crates, sysroot/language crates, and non-lib/bin cargo targets
    /// all keep byte-stable undiscriminated ids (module docs).
    #[must_use]
    pub fn discriminator_for_root(&self, root_file: &Path) -> &TargetDiscriminator {
        self.map.get(root_file).unwrap_or(&TargetDiscriminator::Lib)
    }

    /// Number of recorded lib/bin target roots (diagnostic surface for
    /// the `--hir` pipeline's progress logging).
    #[must_use]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// True when no target roots were recorded (non-cargo workspace).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

/// Shared per-extraction context for the fact-emission chains: the VFS
/// (to correlate a `hir::Crate`'s root file) and the target-root map.
/// Threaded by reference through both emitters' walkers so every
/// id-construction site routes through one discriminator derivation.
pub(crate) struct EmitCtx<'a> {
    pub(crate) vfs: &'a Vfs,
    pub(crate) targets: &'a TargetRootMap,
}

impl EmitCtx<'_> {
    /// The discriminator for `krate` — see [`krate_discriminator`].
    pub(crate) fn discriminator<DB>(&self, db: &DB, krate: Crate) -> TargetDiscriminator
    where
        DB: HirDatabase + Sized,
    {
        krate_discriminator(db, self.vfs, self.targets, krate)
    }
}

/// The [`TargetDiscriminator`] for `krate`, derived by correlating its
/// root file back through the VFS onto the recorded cargo target roots.
/// Shared by the call-site and entry-point emitters so both route ids
/// through one derivation (the #517 split-brain lesson, applied to the
/// discriminator half of identity).
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
        // In-memory / non-filesystem roots (macro-expanded virtual
        // files) cannot be cargo target roots.
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

    /// A recorded bin root resolves to its Bin discriminator.
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

    /// The same-named-bin case (council rust-systems Finding 1): a lib
    /// and a bin whose target name equals the package name are
    /// indistinguishable by origin/display_name — but their ROOT FILES
    /// always differ, so the correlation map separates them.
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

    /// A root absent from the map — non-member dep, sysroot crate, or a
    /// test/example/bench target — falls back to Lib (byte-stable ids).
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

    /// Empty map (non-cargo workspace) — everything Lib.
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
