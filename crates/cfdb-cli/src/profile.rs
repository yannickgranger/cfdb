//! `cfdb extract --profile` phase-timing report.
//!
//! The profile is a diagnostic **gate**, not an optimisation: it attributes
//! `extract`'s wall-clock across six phases so the operator can decide whether
//! incremental-extraction optimisations are worthwhile. v1 ships the
//! measurement only.
//!
//! The breakdown is rendered to **stderr** only — nothing here is written
//! into the keyspace or its canonical dump, so `--profile` cannot perturb
//! determinism.

use std::time::Duration;

/// Per-phase wall-clock breakdown of one `cfdb extract` run: the three
/// `extract`-internal phases surfaced by the Rust extractor (`cargo metadata`,
/// the `syn` walk, deferred resolution) plus the three the CLI orchestrator
/// owns (ingest, the optional `--hir` load, and save).
///
/// `total` is the independently-measured end-to-end wall-clock; the
/// difference between it and [`ExtractProfile::phase_sum`] is the
/// unattributed glue between phases (see [`ExtractProfile::unaccounted`]).
/// The value object holds durations only — it is never serialised into the
/// graph, so it cannot affect determinism.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtractProfile {
    /// `cargo metadata` subprocess.
    pub cargo_metadata: Duration,
    /// `syn` walk — parse, per-file visit, context emission.
    pub syn_walk: Duration,
    /// Post-walk RETURNS / TYPE_OF resolution, synthesis, and canonical sort.
    pub deferred_resolve: Duration,
    /// Node + edge ingest into the store.
    pub ingest: Duration,
    /// The `--hir` `ra_ap_load_cargo` compile plus HIR-fact ingest. `None`
    /// when `--hir` was not requested — the phase did not run.
    pub hir_load: Option<Duration>,
    /// Persisting the keyspace to disk.
    pub save: Duration,
    /// Independently-measured end-to-end wall-clock for the whole extract.
    pub total: Duration,
}

impl ExtractProfile {
    /// Sum of the six phase durations. `hir_load` contributes zero when the
    /// phase did not run. This sum tracks the measured `total` within
    /// tolerance — the only gap is the unattributed glue between phases.
    pub fn phase_sum(&self) -> Duration {
        self.cargo_metadata
            + self.syn_walk
            + self.deferred_resolve
            + self.ingest
            + self.hir_load.unwrap_or_default()
            + self.save
    }

    /// Wall-clock the profile did not attribute to any named phase — the
    /// glue between phases (store construction, dispatch, logging). Saturating
    /// so a phase-sum that marginally exceeds `total` on coarse timers reports
    /// zero rather than underflowing.
    pub fn unaccounted(&self) -> Duration {
        self.total.saturating_sub(self.phase_sum())
    }

    /// Render the breakdown as a human-readable block for stderr: a leading
    /// total, one line per phase with its duration and share of the total,
    /// and a trailing `unaccounted` line. `hir-load` reports
    /// `(skipped; --hir not set)` when the phase did not run.
    pub fn render(&self) -> String {
        let total = self.total.as_secs_f64();
        let mut out = format!("extract phase profile (total {total:.3}s)\n");
        out.push_str(&phase_line("cargo-metadata", self.cargo_metadata, total));
        out.push_str(&phase_line("syn-walk", self.syn_walk, total));
        out.push_str(&phase_line(
            "deferred-resolve",
            self.deferred_resolve,
            total,
        ));
        out.push_str(&phase_line("ingest", self.ingest, total));
        match self.hir_load {
            Some(d) => out.push_str(&phase_line("hir-load", d, total)),
            None => out.push_str(&format!("  {:<16} (skipped; --hir not set)\n", "hir-load")),
        }
        out.push_str(&phase_line("save", self.save, total));
        out.push_str(&phase_line("unaccounted", self.unaccounted(), total));
        out
    }
}

/// Format one `<label> <secs>s <pct>%` row for [`ExtractProfile::render`].
/// `total_secs` guards a zero total (an instant extract) so the percentage
/// never divides by zero.
fn phase_line(label: &str, d: Duration, total_secs: f64) -> String {
    let secs = d.as_secs_f64();
    let pct = if total_secs > 0.0 {
        secs / total_secs * 100.0
    } else {
        0.0
    };
    format!("  {label:<16} {secs:>8.3}s {pct:>5.1}%\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// A profile with no `--hir` phase whose glue is 5ms under a 785ms
    /// total. Shared by the sum/tolerance assertions below.
    fn sample_no_hir() -> ExtractProfile {
        ExtractProfile {
            cargo_metadata: ms(100),
            syn_walk: ms(400),
            deferred_resolve: ms(200),
            ingest: ms(50),
            hir_load: None,
            save: ms(30),
            total: ms(785),
        }
    }

    #[test]
    fn phase_sum_adds_the_six_phases_without_hir() {
        // 100 + 400 + 200 + 50 + 0 (hir None) + 30 = 780.
        assert_eq!(sample_no_hir().phase_sum(), ms(780));
    }

    #[test]
    fn phase_sum_includes_hir_load_when_present() {
        let p = ExtractProfile {
            hir_load: Some(ms(1000)),
            total: ms(1800),
            ..sample_no_hir()
        };
        // 780 + 1000 hir = 1780.
        assert_eq!(p.phase_sum(), ms(1780));
    }

    #[test]
    fn phases_sum_to_the_measured_total_within_tolerance() {
        // RFC-048 §7 Unit: the six phase timers sum to the measured total
        // within tolerance. Here the unattributed glue is 5ms — under 1%
        // of the 785ms total (7.85ms).
        let p = sample_no_hir();
        assert_eq!(p.unaccounted(), ms(5));
        let tolerance = p.total / 100; // 1% of total
        assert!(
            p.unaccounted() <= tolerance,
            "unaccounted {:?} exceeds the 1% tolerance {:?}",
            p.unaccounted(),
            tolerance
        );
    }

    #[test]
    fn unaccounted_saturates_when_phase_sum_exceeds_total() {
        // Coarse timers can make the phase sum marginally exceed the outer
        // total; unaccounted must saturate to zero, not underflow.
        let p = ExtractProfile {
            total: ms(700), // below the 780ms phase sum
            ..sample_no_hir()
        };
        assert_eq!(p.unaccounted(), Duration::ZERO);
    }

    #[test]
    fn render_lists_every_phase_label_and_marks_hir_skipped() {
        let r = sample_no_hir().render();
        for label in [
            "cargo-metadata",
            "syn-walk",
            "deferred-resolve",
            "ingest",
            "save",
            "unaccounted",
        ] {
            assert!(r.contains(label), "render missing `{label}`:\n{r}");
        }
        assert!(
            r.contains("skipped; --hir not set"),
            "a no-hir profile must mark hir-load skipped:\n{r}"
        );
    }

    #[test]
    fn render_shows_hir_load_when_present() {
        let p = ExtractProfile {
            hir_load: Some(ms(1000)),
            total: ms(1800),
            ..sample_no_hir()
        };
        let r = p.render();
        assert!(r.contains("hir-load"), "{r}");
        assert!(
            !r.contains("skipped"),
            "an --hir profile must not print the skipped marker:\n{r}"
        );
    }
}
