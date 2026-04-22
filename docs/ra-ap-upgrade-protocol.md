# `docs/ra-ap-upgrade-protocol.md` — weekly `ra-ap-*` upgrade runbook

**Scope.** Maintenance protocol for the `ra-ap-*` crate family (`ra-ap-hir`, `ra-ap-ide-db`, `ra-ap-syntax`, `ra-ap-rustc_type_ir`, and seven siblings — ten crates total). Consumed by the forthcoming `cfdb-hir-extractor` keystone (Issue #40). This runbook lands first so the upgrade discipline is in place when the dependency arrives.

**Authored per** Issue #39 / RFC-032 §7 Tier 1. Supersedes nothing; extends `docs/cross-fixture-bump.md` with a second weekly maintenance channel distinct from the cross-dogfood pin.

---

## 1. Why a dedicated runbook

### 1.1 Weekly release cadence is not negotiable

`ra-ap-hir` publishes a new `=0.0.N` version **every seven days without exception**. Nine releases landed on crates.io in the nine weeks surveyed at council time (rust-systems verdict B2, April 2026). All ten `ra-ap-*` sub-crates bump together; `ra-ap-rustc_type_ir` additionally releases 2–3× per week on its own axis.

Opting out of the weekly bump means drifting behind HEAD — and because every pin is `=0.0.N` exact, a stale pin is a hard compile failure the next time any consumer pulls a downstream `ra-ap-*` update. **The maintenance tax is budgeted, not optional.**

### 1.2 Exact-version pinning discipline

Every `ra-ap-*` dependency in the workspace `Cargo.toml` uses `=0.0.N` (exact), not `^0.0` or `0.0`. This is load-bearing:

- No version resolution at build time → determinism across developer and CI machines.
- A fresh `cargo build` on a new checkout always gets the exact pinned bundle.
- Upgrades become an explicit, reviewed event on a dedicated chore branch (§2).

When Issue #40 adds the `cfdb-hir-extractor` crate, **every** `ra-ap-*` entry in `crates/cfdb-hir-extractor/Cargo.toml` MUST use the `=0.0.N` form. Any `^`, `~`, or bare `0.0` version range fails the #40 acceptance gate.

---

## 2. The chore-branch protocol

One branch per upgrade. Branch name: `chore/ra-ap-upgrade-<version>` where `<version>` is the new `0.0.N` bundle number (the `ra-ap-hir` version is authoritative; the other nine crates bump in lockstep).

Six steps, enforced in order:

1. **Bump all ten `ra-ap-*` pins simultaneously** in `crates/cfdb-hir-extractor/Cargo.toml` (plus workspace `Cargo.toml` if workspace-level entries are used). Nothing else changes in the same commit.
2. **Bump `ra-ap-rustc_type_ir` to its current HEAD** — this crate's independent 2–3×/week cadence means the version captured in the `ra-ap-hir` 0.0.N bundle is almost always one or two patches behind. Pin to the latest compatible release.
3. **Run the cfdb determinism test suite** (RFC §12.1): `cargo test --workspace --all-features` plus `ci/determinism-check.sh`. Byte-identical canonical dumps across two extracts remain the G1 guarantee and MUST hold.
4. **Run the Pattern B and Pattern C reproduction tests** once they exist (v0.2-2 / v0.2-3 issues — not filed yet at the time of this runbook). Both currently map to the existing arch-ban rule suite as a proxy.
5. **If GREEN** — merge the chore branch. The `cfdb-hir-extractor` CI gate is now allowed to consume the new version. The main CI workflow (running on `develop`) proves it on the next PR.
6. **If RED** — file a `chore(ra-ap): compat issue with 0.0.N` ticket. The pin stays at the last-known-good version. Triage within 7 days (before the next scheduled release supersedes the failing version).

### 2.1 What NEVER goes on a `ra-ap-upgrade` branch

- Any change to `cfdb-core`, `cfdb-extractor`, `cfdb-query`, `cfdb-petgraph`, `cfdb-cli`, or `cfdb-recall` source.
- Any schema changes.
- Any new functionality — even if the new `ra-ap-hir` version "inspired" it.
- Any test additions — if the new version breaks an existing test, that's the failure signal; don't mask it with a new test.

Keep the branch laser-focused: this is pin maintenance, not feature development. If the upgrade forces source changes (breaking API), those source changes land on a separate feature branch that consumes the already-merged pin bump.

---

## 3. Rollback procedure

A pin bump merged that later turns out to break target-workspace rescue runs (or any downstream consumer):

1. Open `chore/ra-ap-rollback-to-<prior-version>`.
2. Set every pin to the prior-known-good version. Commit with rationale: "Rollback: <new-version> caused <symptom> on <consumer>."
3. Run the full test suite + determinism check; if GREEN, merge.
4. File a compat issue against the skipped version so the next weekly bump doesn't attempt it without addressing the root cause.
5. The rollback branch counts as THAT week's upgrade event — the Monday cron does not run a second upgrade in the same week.

Rollback is rare but the procedure is mechanical. Do not "hold at master" or "temporarily downgrade in a feature PR" — every pin change is its own chore branch.

---

## 4. CI automation

### 4.1 Present state

`.gitea/workflows/ra-ap-upgrade.yml` exists as a `workflow_dispatch`-only stub. Manual invocation from the Gitea Actions UI triggers it. The stub today:

- Prints this runbook's §2 protocol and exits 0.
- Does NOT actually edit `Cargo.toml`.
- Does NOT open a PR.

This is intentional: Issue #40 adds the `ra-ap-*` dependencies; without them the workflow has no Cargo.toml entries to edit. The stub exists so the workflow file (and its path) are stable references this runbook can link to from day zero.

### 4.2 Post-#40 wiring

Once the `cfdb-hir-extractor` crate lands, upgrade `.gitea/workflows/ra-ap-upgrade.yml` to:

1. Fetch the latest `ra-ap-hir` version from crates.io.
2. Parse `crates/cfdb-hir-extractor/Cargo.toml` and rewrite every `=0.0.N` pin to the new version.
3. Run the cfdb test suite against the modified tree.
4. On success — commit to `chore/ra-ap-upgrade-<version>` and open a PR via the Gitea API.
5. On failure — open a `chore(ra-ap): compat issue with <version>` issue with the failing test output.

Target schedule after #40: weekly cron, **Wednesday 06:00 UTC** (chosen to avoid Monday 06:00 / 06:30 cross-dogfood jobs and Tuesday 06:00 / 06:30 closed-loop jobs). One weekly slot per job per workday.

### 4.3 Safety rails

The automated workflow MUST NOT:

- Auto-merge its own PRs — human review of the pin diff is mandatory, even when all tests pass. `ra-ap-hir` version bumps occasionally ship behaviour changes that tests don't catch.
- Skip determinism check under any flag or condition.
- Apply source changes. The workflow only edits `Cargo.toml` / `Cargo.lock`.
- Bypass the `cfdb-check` dual-control gate for its own PR.

If any of these rails is violated by a subsequent edit, consider that edit a revert candidate.

---

## 5. Why this protocol exists in `cfdb`, not in `ra-ap-*` upstream

The `ra-ap-*` crates are an unstable, actively-developed slice of `rust-analyzer`'s internals extracted for external consumption. Upstream does not promise API stability between patches. Consumers are expected to track closely and absorb the weekly churn.

cfdb is one such consumer. Other downstream tools (rust-analyzer itself, `cargo-*` plugins, IDE integrations) handle the churn their own way; this runbook is cfdb's.

If cfdb's usage of `ra-ap-*` ever stabilises on a specific long-lived major (e.g. via a future upstream stability commitment or via cfdb adopting a vendored fork), this runbook is retired and replaced with the stability policy for that scheme. Until then, weekly is the floor.

---

## 6. Links

- Issue #39 — this runbook.
- Issue #40 — `cfdb-hir-extractor` keystone (the consumer this runbook serves).
- Addendum §A1.2 — council rust-systems B2 verdict on weekly cadence.
- `docs/cross-fixture-bump.md` — the other weekly maintenance channel (cross-dogfood pin), distinct from this one.

---

## 7. Change history

| Date | Section | Change | PR |
|---|---|---|---|
| 2026-04-19 | all | Initial authoring. CI workflow is a `workflow_dispatch`-only stub pending Issue #40. | #39 |
