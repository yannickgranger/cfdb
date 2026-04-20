//! Cron job fixtures (Issue #126 v0.2-1 coverage gate).
//!
//! Two shapes:
//! - `Job::new_async("<cron>", |_, _| async { ... })` (async variant).
//! - `Job::new("<cron>", |_, _| { ... })` (sync sibling).
//!
//! Stand-ins mirror `tokio_cron_scheduler::Job`. The extractor fires
//! on the path qualifier `Job::` + tail `new_async` / `new`.

pub struct Job;
impl Job {
    pub fn new_async<F>(_cron: &str, _f: F) -> Self {
        Job
    }
    pub fn new<F>(_cron: &str, _f: F) -> Self {
        Job
    }
}

/// First cron job — `Job::new_async` with a minute-granularity cron
/// literal. EXPOSES the enclosing `register_minute_job` fn (closure
/// bodies have no path-level qname).
pub fn register_minute_job() {
    let _j = Job::new_async("0 * * * * *", |_, _| async {});
}

/// Second cron job — `Job::new` (sync variant). Same dispatch arm,
/// exercises the second name in the scanner's tail-match set.
pub fn install_hourly() {
    let _j = Job::new("0 0 * * * *", |_, _| {});
}

/// Control fn — no `Job::new*` call, must NOT be emitted.
pub fn unrelated_setup() {
    let _ = 1 + 1;
}
