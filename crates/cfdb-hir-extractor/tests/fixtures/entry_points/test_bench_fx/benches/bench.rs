//! Bench target — exercises `#[bench]` attribute detection and
//! file-location fallback for an unattributed bench helper.

/// `#[bench]` — must emit `:EntryPoint{kind="bench"}`.
#[bench]
fn bench_one(_b: &mut Bencher) {}

/// No attribute, but lives under `benches/` — file-location detection
/// must emit `:EntryPoint{kind="bench"}`.
fn bench_helper() {}

/// Stand-in for `test::Bencher` so the file parses syntactically. The
/// HIR extractor's probes are textual and don't require real types.
pub struct Bencher;
