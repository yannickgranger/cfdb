//! RFC-041 Phase 1 / Slice 41-C composition root for the
//! `LanguageProducer` registry.
//!
//! This is the **only** module in `cfdb-cli` that names concrete
//! producer types (today: `cfdb_extractor::RustProducer`; later:
//! `cfdb_extractor_php::PhpProducer`, `cfdb_extractor_ts::TypeScriptProducer`).
//! Every other consumer dispatches through `&dyn LanguageProducer`,
//! which keeps the rest of the CLI agnostic to which languages
//! happen to be compiled in.
//!
//! Per clean-arch R1 (RFC-041 §5.1): separating the registry into a
//! dedicated `lang.rs` module — rather than inlining
//! `available_producers()` into `commands/extract.rs` — mirrors the
//! existing `compose::empty_store()` pattern at `commands/extract.rs:52`
//! and keeps the composition-root concern visibly distinct from the
//! `extract` command handler's UX/flag wiring.
//!
//! # Slim-build invariant
//!
//! Under `cargo install cfdb-cli --no-default-features` (zero
//! `lang-*` features), `available_producers()` returns an empty
//! `Vec`. `cfdb extract --workspace <any>` then returns
//! [`NoProducerDetected`] cleanly — no panic, no silent success.
//! The slim-build CI step at `.gitea/workflows/ci.yml` verifies
//! `cargo check -p cfdb-cli --no-default-features` compiles with
//! NO `cfdb-extractor` in the dep tree, so the `syn` /
//! `cargo_metadata` / `ra-ap-*` transitives are dropped entirely.

use cfdb_lang::LanguageProducer;
use thiserror::Error;

/// Build the static registry of producers compiled in by feature flags.
///
/// Every concrete producer crate is gated behind a `lang-<name>`
/// feature on `cfdb-cli`. The bare-name vs `dep:`-prefix distinction
/// is load-bearing under resolver v2 (per RFC-041 §3.4 + rust-systems
/// R2 factual correction): the `[features]` block's
/// `lang-rust = ["dep:cfdb-extractor"]` form is the one that
/// actually gates the optional dep.
// `vec_init_then_push` is the canonical idiom for feature-gated
// registries — `vec![<cfg-gated entries>]` is structurally awkward
// because conditional macro arms aren't supported. Suppress the lint
// for this fn only.
#[allow(clippy::vec_init_then_push)]
pub(crate) fn available_producers() -> Vec<Box<dyn LanguageProducer>> {
    // `mut` is conditionally needed (only when at least one
    // `lang-*` feature is on); without the `cfg_attr` the slim
    // build emits an `unused_mut` warning on the empty `Vec::new()`.
    #[cfg_attr(
        not(any(
            feature = "lang-rust",
            feature = "lang-php",
            feature = "lang-typescript"
        )),
        allow(unused_mut)
    )]
    let mut v: Vec<Box<dyn LanguageProducer>> = Vec::new();
    #[cfg(feature = "lang-rust")]
    v.push(Box::new(cfdb_extractor::RustProducer));
    #[cfg(feature = "lang-php")]
    v.push(Box::new(cfdb_extractor_php::PhpProducer));
    #[cfg(feature = "lang-typescript")]
    v.push(Box::new(cfdb_extractor_ts::TypeScriptProducer));
    v
}

/// `cfdb extract` was invoked but no compiled-in producer accepted
/// the workspace.
///
/// Carries the workspace path + the names of producers that were
/// compiled in (so the user knows whether the build is slim, has
/// only `lang-rust` enabled, etc.). A typical message:
///
/// ```text
/// no LanguageProducer detected workspace "/tmp/some-rails-app";
///   compiled-in producers: ["rust"]
/// ```
///
/// Mapped to a `CfdbCliError::NoProducer` variant via `#[from]` so
/// the existing `?`-propagation idiom in command handlers keeps
/// working unchanged.
#[derive(Debug, Error)]
#[error(
    "no LanguageProducer detected workspace `{workspace}`; \
     compiled-in producers: {compiled_in:?}"
)]
pub struct NoProducerDetected {
    pub workspace: String,
    pub compiled_in: Vec<&'static str>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// With default features (`lang-rust` on, `lang-php` /
    /// `lang-typescript` OFF), the registry contains exactly one
    /// producer named `"rust"`. Catches a feature-flag regression
    /// that accidentally drops `lang-rust` from defaults OR
    /// accidentally promotes `lang-php` / `lang-typescript` into
    /// the default set without a documented decision. Tightened in
    /// RFC-041 Phase 2/3 (#264 / #265) bundle PR — when
    /// `lang-rust + lang-php + lang-typescript` are all on (the
    /// `--features lang-rust,lang-php,lang-typescript` build path),
    /// the registry has 3 entries and this assertion would
    /// false-positive; the cfg gates the test to default-only.
    #[test]
    #[cfg(all(
        feature = "lang-rust",
        not(feature = "lang-php"),
        not(feature = "lang-typescript")
    ))]
    fn default_features_register_only_rust_producer() {
        let producers = available_producers();
        let names: Vec<&'static str> = producers.iter().map(|p| p.name()).collect();
        assert_eq!(names, vec!["rust"]);
    }

    /// Under the all-languages build (`--features lang-rust,lang-php,
    /// lang-typescript`), the registry contains all three producers
    /// in declaration order: `["rust", "php", "typescript"]`. Catches
    /// a regression in `available_producers()` ordering or a missing
    /// `#[cfg]` arm.
    #[test]
    #[cfg(all(
        feature = "lang-rust",
        feature = "lang-php",
        feature = "lang-typescript"
    ))]
    fn all_languages_register_in_declaration_order() {
        let producers = available_producers();
        let names: Vec<&'static str> = producers.iter().map(|p| p.name()).collect();
        assert_eq!(names, vec!["rust", "php", "typescript"]);
    }

    /// Under slim build (no `lang-*` feature), the registry is
    /// empty. This compiles only when ALL `lang-*` features are off
    /// — the CI's slim-build step (`cargo check -p cfdb-cli
    /// --no-default-features`) exercises this path.
    #[test]
    #[cfg(not(any(feature = "lang-rust")))]
    fn slim_build_registry_is_empty() {
        let producers = available_producers();
        assert!(
            producers.is_empty(),
            "slim build (no lang-* features) must produce an empty registry"
        );
    }
}
