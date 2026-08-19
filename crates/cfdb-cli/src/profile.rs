use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExtractProfile {
    pub cargo_metadata: Duration,
    pub syn_walk: Duration,
    pub deferred_resolve: Duration,
    pub ingest: Duration,
    pub hir_load: Option<Duration>,
    pub save: Duration,
    pub total: Duration,
}

impl ExtractProfile {
    pub fn phase_sum(&self) -> Duration {
        self.cargo_metadata
            + self.syn_walk
            + self.deferred_resolve
            + self.ingest
            + self.hir_load.unwrap_or_default()
            + self.save
    }

    pub fn unaccounted(&self) -> Duration {
        self.total.saturating_sub(self.phase_sum())
    }

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
        assert_eq!(sample_no_hir().phase_sum(), ms(780));
    }

    #[test]
    fn phase_sum_includes_hir_load_when_present() {
        let p = ExtractProfile {
            hir_load: Some(ms(1000)),
            total: ms(1800),
            ..sample_no_hir()
        };
        assert_eq!(p.phase_sum(), ms(1780));
    }

    #[test]
    fn phases_sum_to_the_measured_total_within_tolerance() {
        let p = sample_no_hir();
        assert_eq!(p.unaccounted(), ms(5));
        let tolerance = p.total / 100;
        assert!(
            p.unaccounted() <= tolerance,
            "unaccounted {:?} exceeds the 1% tolerance {:?}",
            p.unaccounted(),
            tolerance
        );
    }

    #[test]
    fn unaccounted_saturates_when_phase_sum_exceeds_total() {
        let p = ExtractProfile {
            total: ms(700),
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
