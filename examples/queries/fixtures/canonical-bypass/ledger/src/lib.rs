// Canonical-bypass fixture — the `ledger` crate.
//
// Declares `LedgerRepository` with BOTH the canonical
// (`append_idempotent`) and the non-canonical (`append`) wire-level
// methods. The `LedgerService` facade then invokes these from four
// distinct call sites, each one designed to surface under exactly one of
// the four Pattern C verdicts when the rule runs against this tree:
//
//   CANONICAL_CALLER      — record_trade_safe() invokes append_idempotent
//   BYPASS_REACHABLE      — record_trade()      invokes append    (reached via cli::record)
//   BYPASS_DEAD           — record_orphan()     invokes append    (no entry point reaches it)
//   CANONICAL_UNREACHABLE — record_isolated()   invokes append_idempotent but nothing reaches it
//
// The concepts TOML declares `ledger` as the canonical crate, so every
// :Item in this crate (including record_isolated) gets a CANONICAL_FOR
// edge. `enrich_reachability` then marks record_isolated as unreachable
// because the CLI never calls into it — satisfying the CANONICAL_UNREACHABLE
// rule which keys on (:Item CANONICAL_FOR :Concept) with
// reachable_from_entry=false.

pub struct LedgerService<R> {
    pub repo: R,
}

pub trait LedgerRepository {
    // Non-canonical form (the bypass). #3525-class bug shape.
    fn append(&self, entries: Vec<i64>);
    // Canonical form (the safety-barrier wire-level method).
    fn append_idempotent(&self, external_ref: &str, entries: Vec<i64>);
}

impl<R: LedgerRepository> LedgerService<R> {
    // The live #3525 bug shape. Reached from the CLI entry point below.
    // Surfaces as BYPASS_REACHABLE.
    pub fn record_trade(&self) {
        let entries = build_entries();
        self.repo.append(entries);
    }

    // The canonical caller — surfaces as CANONICAL_CALLER. Also reached
    // from the CLI; the canonical form is the "OK" wiring shape.
    pub fn record_trade_safe(&self) {
        let entries = build_entries();
        self.repo.append_idempotent("trade-ref", entries);
    }

    // A bypass caller that NO entry point reaches. #3544/#3545 /
    // #3546-class shape: the call site exists, the parser/resolver path
    // scatters across multiple functions, but this particular variant
    // is stranded behind an unwired helper. Surfaces as BYPASS_DEAD.
    pub fn record_orphan(&self) {
        let entries = build_entries();
        self.repo.append(entries);
    }

    // A canonical impl that no entry point reaches — surfaces as
    // CANONICAL_UNREACHABLE. Models the #1526 shape where a safety
    // envelope exists and is correct, but is wired around instead of
    // through. The method IS a resolver of the concept, lives in the
    // canonical crate (so carries CANONICAL_FOR via enrich_concepts),
    // and has no entry-point reachability.
    pub fn record_isolated(&self) {
        let entries = build_entries();
        self.repo.append_idempotent("isolated-ref", entries);
    }
}

fn build_entries() -> Vec<i64> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test-only harness that bypasses the canonical — the `is_test`
    // filter on the bypass rules MUST drop this row (tests legitimately
    // exercise wire-level forms to build scenarios).
    pub struct MockRepo;
    impl LedgerRepository for MockRepo {
        fn append(&self, _entries: Vec<i64>) {}
        fn append_idempotent(&self, _r: &str, _e: Vec<i64>) {}
    }

    impl LedgerService<MockRepo> {
        pub fn seed_fixture(&self) {
            self.repo.append(Vec::new());
        }
    }
}
