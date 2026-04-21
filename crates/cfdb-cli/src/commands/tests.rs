//! Unit tests for the command handlers. Split out of `commands.rs`
//! verbatim for the drift god-file decomposition (#151).

use std::path::PathBuf;

use super::extract::{
    cache_base_dir, cache_dir_for, is_url_at_sha, parse_url_at_sha, short_rev, url_hash_hex16,
};

#[test]
fn short_rev_truncates_long_sha() {
    assert_eq!(
        short_rev("abcdef0123456789abcdef0123456789abcdef01"),
        "abcdef012345"
    );
}

#[test]
fn short_rev_preserves_short_sha_or_branch_name() {
    assert_eq!(short_rev("abc123"), "abc123");
    assert_eq!(short_rev("main"), "main");
    assert_eq!(short_rev("v0.2.3"), "v0.2.3");
}

#[test]
fn short_rev_sanitises_path_unsafe_chars() {
    assert_eq!(short_rev("feature/new-thing"), "feature_new-thing");
    assert_eq!(short_rev("release candidate"), "release_candidate");
}

#[test]
fn short_rev_keeps_non_hex_long_names_verbatim() {
    // A tag like `v0.1.0-beta2` is longer than 12 chars but not
    // hex-only — should be kept as-is (not truncated).
    assert_eq!(short_rev("v0.1.0-beta2"), "v0.1.0-beta2");
}

// ---------------------------------------------------------------------
// Option W: <url>@<sha> parsing (issue #96 / RFC-032 §A1.7).
// These are the four discriminations the match guard in `extract`
// relies on — mirror them in the Wiring Assertion section of
// `.prescriptions/96.md`.
// ---------------------------------------------------------------------

#[test]
fn parse_url_at_sha_accepts_https_with_full_hex_sha() {
    assert_eq!(
        parse_url_at_sha("https://host/repo@abcdef0123456789abcdef0123456789abcdef01"),
        Some((
            "https://host/repo",
            "abcdef0123456789abcdef0123456789abcdef01",
        ))
    );
    assert!(is_url_at_sha(
        "https://host/repo@abcdef0123456789abcdef0123456789abcdef01"
    ));
}

#[test]
fn parse_url_at_sha_accepts_http_and_ssh_schemes() {
    assert!(is_url_at_sha("http://host/repo@deadbeef"));
    assert!(is_url_at_sha("ssh://host/repo@deadbeef"));
    // https covered in the previous test.
}

#[test]
fn parse_url_at_sha_splits_on_rightmost_at_sign() {
    // SSH-shorthand embedded before the URL@SHA separator — the SHA
    // side must be hex, so `git@host.com:path@deadbeef` parses.
    // (The URL side `git@host.com:path` is then rejected for lacking
    //  a recognised scheme — v1 does NOT accept SSH shorthand.)
    assert_eq!(parse_url_at_sha("git@host.com:path@deadbeef"), None);
    // But an explicit `ssh://` prefix that ALSO carries an `@` in the
    // host part must split on the rightmost `@`:
    assert_eq!(
        parse_url_at_sha("ssh://user@host.com/repo@deadbeef"),
        Some(("ssh://user@host.com/repo", "deadbeef"))
    );
}

#[test]
fn parse_url_at_sha_rejects_plain_sha() {
    // A plain SHA (no `@`) must fall through to the same-repo path.
    assert_eq!(parse_url_at_sha("abc123"), None);
    assert_eq!(
        parse_url_at_sha("abcdef0123456789abcdef0123456789abcdef01"),
        None
    );
    assert!(!is_url_at_sha("abc123"));
}

#[test]
fn parse_url_at_sha_accepts_file_scheme() {
    // file:// is accepted for hermetic tests + self-dogfood use case
    // `cfdb extract --rev file://$(pwd)/.git@$(git rev-parse HEAD)`.
    assert_eq!(
        parse_url_at_sha("file:///tmp/r@deadbeef"),
        Some(("file:///tmp/r", "deadbeef"))
    );
    assert!(is_url_at_sha("file:///tmp/r@deadbeef"));
}

#[test]
fn parse_url_at_sha_rejects_unknown_scheme() {
    // URL side must carry http:// / https:// / ssh:// / file://.
    assert_eq!(parse_url_at_sha("ftp://host/r@deadbeef"), None);
    assert_eq!(parse_url_at_sha("rsync://host/r@deadbeef"), None);
    assert!(!is_url_at_sha("ftp://host/r@deadbeef"));
}

#[test]
fn parse_url_at_sha_rejects_non_hex_or_short_sha() {
    // Post-`@` side must be all-hex and ≥ 7 chars.
    assert_eq!(parse_url_at_sha("https://host/r@notahex"), None);
    assert_eq!(parse_url_at_sha("https://host/r@abc"), None); // too short (3 < 7)
                                                              // Mixed case hex IS valid (git accepts it).
    assert!(is_url_at_sha("https://host/r@AbCdEf0"));
}

#[test]
fn url_hash_hex16_is_deterministic_and_16_chars() {
    let a = url_hash_hex16("https://example.com/repo");
    let b = url_hash_hex16("https://example.com/repo");
    assert_eq!(a, b, "same input must hash identically");
    assert_eq!(a.len(), 16, "hash must be exactly 16 hex chars");
    assert!(
        a.chars().all(|c| c.is_ascii_hexdigit()),
        "hash must be all hex: got `{a}`"
    );
    // Different URLs produce different hashes.
    let c = url_hash_hex16("https://example.com/other");
    assert_ne!(a, c);
}

#[test]
fn cache_dir_for_structure_is_base_slash_hex16_slash_sha() {
    // Use CFDB_CACHE_DIR override to make the base deterministic and
    // avoid touching the user's actual HOME/XDG_CACHE_HOME. The
    // env_lock serialises against other env-var tests.
    let _guard = env_lock();
    let base = std::env::temp_dir().join("cfdb-test-cache-96");
    // SAFETY: single-threaded inside env_lock().
    unsafe { std::env::set_var("CFDB_CACHE_DIR", &base) };
    let sha = "abcdef0123456789abcdef0123456789abcdef01";
    let dir = cache_dir_for("https://example.com/repo", sha);
    assert_eq!(dir.parent().and_then(|p| p.parent()), Some(base.as_path()));
    assert_eq!(
        dir.file_name().and_then(|s| s.to_str()),
        Some(sha),
        "innermost dir must be full SHA"
    );
    let hex = dir
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|s| s.to_str())
        .expect("url-hash segment");
    assert_eq!(hex.len(), 16);
    assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
    unsafe { std::env::remove_var("CFDB_CACHE_DIR") };
}

#[test]
fn cache_base_dir_env_var_precedence() {
    let _guard = env_lock();
    // Save original env so we can restore.
    let orig_cfdb = std::env::var_os("CFDB_CACHE_DIR");
    let orig_xdg = std::env::var_os("XDG_CACHE_HOME");
    let orig_home = std::env::var_os("HOME");

    // CFDB_CACHE_DIR wins outright.
    unsafe {
        std::env::set_var("CFDB_CACHE_DIR", "/tmp/cfdb-explicit");
        std::env::set_var("XDG_CACHE_HOME", "/tmp/xdg");
        std::env::set_var("HOME", "/tmp/home");
    }
    assert_eq!(cache_base_dir(), PathBuf::from("/tmp/cfdb-explicit"));

    // XDG_CACHE_HOME is used when CFDB_CACHE_DIR is unset.
    unsafe { std::env::remove_var("CFDB_CACHE_DIR") };
    assert_eq!(cache_base_dir(), PathBuf::from("/tmp/xdg/cfdb/extract"));

    // HOME is used when both CFDB_* and XDG_* are unset.
    unsafe { std::env::remove_var("XDG_CACHE_HOME") };
    assert_eq!(
        cache_base_dir(),
        PathBuf::from("/tmp/home/.cache/cfdb/extract")
    );

    // Restore original env.
    unsafe {
        match orig_cfdb {
            Some(v) => std::env::set_var("CFDB_CACHE_DIR", v),
            None => std::env::remove_var("CFDB_CACHE_DIR"),
        }
        match orig_xdg {
            Some(v) => std::env::set_var("XDG_CACHE_HOME", v),
            None => std::env::remove_var("XDG_CACHE_HOME"),
        }
        match orig_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
    }
}

/// Serialise env-var-touching tests so they don't race each other in
/// `cargo test`'s default parallel harness. Intentionally uses a
/// hand-rolled `Mutex` rather than pulling in `serial_test` — keeps
/// the dep list minimal (forbidden move intersection: no new deps
/// beyond `sha2`).
fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}
