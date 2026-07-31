//! `cfdb-lang` — the producer-side seam for cfdb's multi-language story.
//!
//! Defines the [`LanguageProducer`] trait + [`LanguageError`] enum that
//! every language-specific producer (Rust today via `cfdb-extractor`,
//! PHP and TypeScript follow via #264 / #265) implements. Per RFC-041:
//!
//! - **Bounded context: language production.** Distinct from
//!   `cfdb-core`'s schema-vocabulary context (which owns the
//!   `:Item` / `:CallSite` / `:Module` / `:Crate` types + the
//!   `IN_*` / `INVOKES_AT` edge labels) and from `cfdb-extractor`'s
//!   Rust-producer context (which owns the syn walker + the
//!   `Cargo.toml`-based detector). `cfdb-lang` is the *seam* between
//!   them — the published port that consumer-side code (the CLI
//!   dispatcher in `cfdb-cli/src/lang.rs`) calls into without naming
//!   any concrete producer crate.
//! - **Crate stability profile.** Sits next to `cfdb-core` in the
//!   inner ring. Phase 1 instability metrics
//!   (per RFC-041 §3.2 SAP table): Ca = 2 (`cfdb-extractor` +
//!   `cfdb-cli`), Ce = 1 (`cfdb-core`), I = 0.33, A = 0.50, D = 0.17
//!   (Zone of Usefulness). Adding more producers (#264, #265)
//!   decreases instability.
//! - **NOT a schema vocabulary owner.** Concrete producers MUST
//!   emit only the closed-set `:Item.kind` values declared in
//!   `cfdb-core::schema::labels` (the Rust producer's existing
//!   `"struct"` / `"enum"` / `"trait"` / `"fn"` / `"impl_block"` /
//!   `"const"` / `"static"` / `"type"` / `"mod"`). New `kind`
//!   values for future languages require a separate schema RFC +
//!   `cfdb-core::SchemaVersion` patch + lockstep PR on
//!   `graph-specs-rust` per RFC-033 §4 I2. The trait does NOT
//!   widen the schema by side effect — Published Language
//!   invariant per RFC-041 §4.

use std::path::{Path, PathBuf};

use cfdb_core::fact::{Edge, Node};

/// A language-specific producer of cfdb structural facts.
///
/// Each implementation walks a workspace whose root path matches its
/// detection criterion and emits the v0.1 `:Item` / `:CallSite` /
/// `:Module` / `:Crate` / `IN_*` / `INVOKES_AT` fact set defined in
/// `cfdb-core::schema`. The set of allowed `:Item.kind` values is
/// schema-governed (see crate-root docs); a producer that needs a new
/// `kind` must ship a separate RFC + `cfdb-core::schema` patch, not
/// extend the open-set ad-hoc.
///
/// # Object safety
///
/// The trait is object-safe under the standard conditions:
/// - no generic methods,
/// - no `where Self: Sized` clauses on any method,
/// - all method receivers are `&self`,
/// - no associated types.
///
/// `Box<dyn LanguageProducer>` is the dispatch shape used by
/// `cfdb-cli`'s `available_producers()` registry.
///
/// # Supertrait bound
///
/// `Send` only. The v0.1 dispatcher is single-threaded sequential —
/// `Sync` is not required and would impose an unnecessary constraint
/// on future producer implementers. Re-add `Sync` only when a
/// polyglot-parallel-dispatch design surfaces (deferred per RFC-041
/// §6).
pub trait LanguageProducer: Send {
    /// Stable kebab-case identifier used in CLI flags + keyspace
    /// suffixes (`"rust"`, `"php"`, `"typescript"`). Must match the
    /// Cargo feature gate (`lang-<name>`) on `cfdb-cli`.
    fn name(&self) -> &'static str;

    /// `true` when this producer is willing to walk `workspace_root`.
    ///
    /// MUST be cheap — typically reads one or two marker files
    /// (`Cargo.toml` for Rust, `composer.json` for PHP,
    /// `package.json` + `tsconfig.json` for TypeScript). MUST NOT
    /// walk the entire workspace; `detect()` runs once per producer
    /// per CLI invocation, gating the expensive `produce()` call.
    fn detect(&self, workspace_root: &Path) -> bool;

    /// Walk the workspace and emit the fact set.
    ///
    /// Pure: produces the node + edge vectors and returns; does not
    /// touch any store. Errors carry a [`LanguageError`] enumerating
    /// the failure modes the producer recognises. The method name
    /// `produce` (rather than `extract`) is deliberate — `extract`
    /// is overloaded across the CLI verb (`cfdb extract`), the
    /// crate name (`cfdb-extractor`), and the legacy fn
    /// (`cfdb_extractor::extract_workspace`); the trait method
    /// uses the unambiguous verb instead (per RFC-041 §3.1 ddd
    /// homonym ruling).
    fn produce(&self, workspace_root: &Path) -> Result<(Vec<Node>, Vec<Edge>), LanguageError>;
}

/// Errors a [`LanguageProducer`] may surface.
///
/// The producer name field on `NotDetected` and `Parse` is
/// `&'static str` because every concrete producer's `name()` returns
/// a static string slice — propagating it through the error preserves
/// the diagnostic chain without forcing heap allocation on the error
/// path.
#[derive(Debug, thiserror::Error)]
pub enum LanguageError {
    /// `produce()` was invoked on a workspace the producer's
    /// `detect()` would have rejected. The CLI dispatcher prevents
    /// this by gating `produce()` behind a `detect()` check; this
    /// variant exists for callers who bypass the dispatcher (tests,
    /// embedding).
    #[error("workspace root not detected by producer `{producer}`: {reason}")]
    NotDetected {
        producer: &'static str,
        reason: String,
    },

    /// I/O failure walking the workspace — unreadable directory,
    /// missing marker file at a path the producer expected.
    #[error("workspace root I/O failed: {0}")]
    Io(#[from] std::io::Error),

    /// Producer-specific parse failure — for the Rust producer this
    /// wraps `cfdb_extractor::ExtractError`'s rendering; for future
    /// producers it'd wrap their parser's error message. The string
    /// shape preserves the message without forcing every caller to
    /// downcast on the original error type (the trait is object-safe;
    /// consumers cannot generically introspect concrete error types
    /// through `&dyn LanguageProducer`).
    #[error("producer-specific parse failure in `{producer}`: {message}")]
    Parse {
        producer: &'static str,
        message: String,
    },

    /// A walked file fell outside the canonical workspace root
    /// (issue #540, the #527 dead-fence class). Every `file` prop on
    /// an emitted fact is contractually workspace-relative; a
    /// `strip_prefix` miss therefore surfaces loudly instead of
    /// silently shipping an absolute path that no file-scoped fence
    /// anchored on a relative path can ever match.
    #[error(
        "producer `{producer}`: file `{file}` is outside the canonical \
         workspace root `{workspace_root}` — refusing to emit a \
         non-workspace-relative `file` prop"
    )]
    PathNotInWorkspace {
        producer: &'static str,
        file: PathBuf,
        workspace_root: PathBuf,
    },
}

/// Canonicalize a workspace root once at `produce()` entry.
///
/// The single canonical resolver for the #527 / #540 workspace-root bug
/// class: producers MUST resolve the caller-spelled root (`.`, a
/// relative path, a path through a symlink, a `..`-terminated form)
/// through this before deriving any fact from it — directory-name-based
/// crate names and workspace-relative `file` props both silently corrupt
/// on non-canonical roots otherwise.
///
/// # Errors
///
/// `LanguageError::Io` when the root does not exist or cannot be
/// resolved.
pub fn canonical_workspace_root(workspace_root: &Path) -> Result<PathBuf, LanguageError> {
    Ok(workspace_root.canonicalize()?)
}

/// Compute the workspace-relative form of `file_path`, loudly.
///
/// `canonical_root` MUST come from [`canonical_workspace_root`]. A
/// mismatch is an error, never a silent absolute-path fallback — the
/// #527 lesson: a fact whose `file` prop violates the
/// "always workspace-relative" contract turns every file-scoped fence
/// anchored on a relative path into a silently dead rule.
///
/// # Errors
///
/// [`LanguageError::PathNotInWorkspace`] when `file_path` does not live
/// under `canonical_root`.
pub fn workspace_relative(
    file_path: &Path,
    canonical_root: &Path,
    producer: &'static str,
) -> Result<String, LanguageError> {
    Ok(file_path
        .strip_prefix(canonical_root)
        .map_err(|_| LanguageError::PathNotInWorkspace {
            producer,
            file: file_path.to_path_buf(),
            workspace_root: canonical_root.to_path_buf(),
        })?
        .to_string_lossy()
        .into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Object-safety pin per RFC-041 §7 Slice 41-A. If the trait
    /// stops being object-safe (e.g. someone adds a generic method
    /// or an associated type), this fails to compile.
    #[test]
    fn language_producer_is_object_safe() {
        fn _assert_obj_safe(_: &dyn LanguageProducer) {}
    }

    /// `Send` bound pin — accidental `Sync` reintroduction would
    /// silently constrain future implementers; accidental loss of
    /// `Send` would prevent shipping the trait object across thread
    /// boundaries when a future polyglot-parallel-dispatch design
    /// arrives. Pin both directions here.
    #[test]
    fn language_producer_is_send() {
        fn _assert_send_box(_: Box<dyn LanguageProducer>)
        where
            Box<dyn LanguageProducer>: Send,
        {
        }
    }

    #[test]
    fn workspace_relative_strips_the_canonical_root() {
        let root = Path::new("/ws/project");
        let rel = workspace_relative(Path::new("/ws/project/src/a.php"), root, "php")
            .expect("in-workspace path must resolve");
        assert_eq!(rel, "src/a.php");
    }

    #[test]
    fn workspace_relative_refuses_paths_outside_the_root() {
        let root = Path::new("/ws/project");
        let err = workspace_relative(Path::new("/elsewhere/b.ts"), root, "typescript")
            .expect_err("out-of-workspace path must be a loud error, never a silent absolute");
        assert!(matches!(
            err,
            LanguageError::PathNotInWorkspace {
                producer: "typescript",
                ..
            }
        ));
    }

    #[test]
    fn canonical_workspace_root_refuses_missing_roots() {
        let err = canonical_workspace_root(Path::new("definitely/not/a/dir"))
            .expect_err("missing root must error");
        assert!(matches!(err, LanguageError::Io(_)));
    }
}
