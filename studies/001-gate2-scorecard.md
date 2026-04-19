# Gate 2 — Repo quality scorecard

**Date:** 2026-04-13
**Candidates scored:** LadybugDB, CozoDB, SurrealDB, petgraph
**Candidates not scored (dropped at Gate 1):** DuckDB+DuckPGQ, Oxigraph
**Method:** GitHub REST API at https://api.github.com/ (unauthenticated, 60 req/hr core + 10 req/hr search), augmented by local `git clone --bare --filter=blob:none` of each repo for commit/tag/tree analysis once the API core budget was exhausted. Search API used for issue ratios; WebFetch used for HTML-rendered pages (CHANGELOG probes, security advisories, docs.rs coverage, CI badge SVGs). Time windows anchored at 2026-04-13 (30d = since 2026-03-14, 90d = since 2026-01-13, 12m = since 2025-04-13). Raw numbers in the cells; weighted score at the bottom.

## Repo identities

| Candidate | Primary repo | Rust crate repo (if different) |
|---|---|---|
| LadybugDB | `LadybugDB/ladybug` | `lbugdb/lbug` (returns 404 — see Raw API response notes) |
| CozoDB | `cozodb/cozo` | (same) |
| SurrealDB | `surrealdb/surrealdb` | (same) |
| petgraph | `petgraph/petgraph` | (same) |

## Activity metrics (40% weight)

| Metric | Weight | Threshold | LadybugDB | CozoDB | SurrealDB | petgraph |
|---|---|---|---|---|---|---|
| A1 — Commits last 30d | 10% | ≥ 10 | 88 | 0 | 50 | 0 |
| A2 — Commits last 90d | 10% | ≥ 30 | 292 | 0 | 228 | 3 |
| A3 — Distinct contributors 12m | 10% | ≥ 3 | 38 | 0 | 58 | 28 |
| A4 — Tagged releases last 12m | 10% | ≥ 4 | 13 | 0 | 24+ | 2 |

## Stability metrics (30% weight)

| Metric | Weight | Threshold | LadybugDB | CozoDB | SurrealDB | petgraph |
|---|---|---|---|---|---|---|
| S1 — Stars | 5% | ≥ 500 | 933 | 3949 | 31835 | 3844 |
| S2 — Semver discipline | 5% | yes | yes (v0.15.3, v0.15.2, v0.15.1, v0.15.0, v0.14.1) | yes (v0.7.6, v0.7.5, v0.7.3-beta1, v0.7.2, v0.7.1) | yes (v3.0.5, v2.6.5, v2.6.4, v3.0.4, v3.0.3) | yes (petgraph@v0.8.3, v0.8.2, v0.8.1, v0.8.0, v0.7.1) |
| S3 — Open CVEs | 10% | 0 | 0 | 0 | 10+ (published GHSAs, patched status unconfirmed on index page) | 0 |
| S4 — Open/closed issue ratio 12m (open/(open+closed)) | 5% | ≤ 0.5 | 59/138 = 0.428 | 4/4 = 1.000 | 197/453 = 0.435 | 46/66 = 0.697 |
| S5 — License compatible (no AGPL/BSL/SSPL) | 5% | yes | MIT | MPL-2.0 | BSL-1.1 (fail — incompatible with cfdb embedded-in-other-projects deployment) | Apache-2.0 (dual MIT) |

## Architecture metrics (30% weight)

| Metric | Weight | Threshold | LadybugDB | CozoDB | SurrealDB | petgraph |
|---|---|---|---|---|---|---|
| H1 — CHANGELOG present | 5% | yes | no | no | no | yes (CHANGELOG.md at root) |
| H2 — CI configured + passing | 5% | yes | yes (18 workflows; ci-workflow.yml badge = passing) | configured (build.yml) but no passing runs on main since 2024-12-04; recent PR runs show "Action required" (no maintainer) | configured (13 workflows) but ci.yml badge = failing on main | yes (ci.yml badge = passing on master) |
| H3 — Coverage visible | 5% | yes | no (`.lcovrc` present and `BUILD_LCOV` CMake option exists, but no coverage step in CI and no badge in README) | no | yes (dedicated `coverage.yml` workflow uploads to Codecov via `cargo-llvm-cov`) | no |
| H4 — docs.rs coverage | 5% | ≥ 50% | 61.17% (lbug 0.15.3: 63/103 items) | 100% (cozo 0.7.6: 134/134 items) | 89.52% (surrealdb: 222/248 items) | 79.17% (petgraph: 456/576 items) |
| H5 — LOC (Rust+C+C++) | 5% | < 500k | ~251k (C++ 7.45M + C 85k bytes ÷ 30 — C++ dominant) | ~74k (Rust 2.21M bytes ÷ 30) | ~356k (Rust 10.69M bytes ÷ 30) | ~43k (Rust 1.30M bytes ÷ 30) |
| H6 — Bus factor (top-author share, commits last 12m) | 5% | < 80% | 42.8% (Arun Sharma: 416/972) | N/A (0 commits 12m) | 21.3% (Stu Schwartz: 137/644) | 43.7% (Raoul Luqué: 31/71) |

## Pass/fail matrix (1 = meets threshold, 0 = does not)

| Metric | Weight | LadybugDB | CozoDB | SurrealDB | petgraph |
|---|---|---|---|---|---|
| A1 | 10% | 1 | 0 | 1 | 0 |
| A2 | 10% | 1 | 0 | 1 | 0 |
| A3 | 10% | 1 | 0 | 1 | 1 |
| A4 | 10% | 1 | 0 | 1 | 0 |
| S1 | 5% | 1 | 1 | 1 | 1 |
| S2 | 5% | 1 | 1 | 1 | 1 |
| S3 | 10% | 1 | 1 | 0 | 1 |
| S4 | 5% | 1 | 0 | 1 | 0 |
| S5 | 5% | 1 | 1 | 0 | 1 |
| H1 | 5% | 0 | 0 | 0 | 1 |
| H2 | 5% | 1 | 0 | 0 | 1 |
| H3 | 5% | 0 | 0 | 1 | 0 |
| H4 | 5% | 1 | 1 | 1 | 1 |
| H5 | 5% | 1 | 1 | 1 | 1 |
| H6 | 5% | 1 | 0 | 1 | 1 |
| **TOTAL** | **100%** | **90%** | **35%** | **75%** | **60%** |

## Summary

| Candidate | Weighted score | Threshold (≥60%) | Gate 2 verdict |
|---|---|---|---|
| LadybugDB | 90% | met | **ADVANCE** |
| CozoDB | 35% | not met | **DROP** |
| SurrealDB | 75% | met (numerically) | **DROP (license blocker)** — S5 is a hard fail for cfdb (BSL-1.1 forbids embedding in other projects as a hosted DBaaS equivalent; Apache-2.0 conversion date is 2030-01-01). Even though the numeric score clears 60%, the methodology's S5 threshold is binary: AGPL / BSL / SSPL is a drop-regardless-of-other-rows rule. If cfdb's usage model (in-process embedded graph store inside a solo dev's Rust code-facts tool) is later confirmed by legal review to fall outside the BSL Additional Use Grant, SurrealDB can be reinstated via a methodology amendment and a belated Gate 3 spike. |
| petgraph | 60% | met (exactly at threshold) | **ADVANCE** (as fallback anchor per §5.3) |

**Gate 3 candidates: LadybugDB + petgraph-baseline** (2 survivors).

## Methodology caveats

- Unauthenticated GitHub API core rate limit (60 req/hr) was exhausted roughly halfway through data collection. Remaining metrics (commit totals past 100-commit page, tag dates, tree listings, CHANGELOG probes, contributor distribution for H6) were collected by `git clone --bare --filter=blob:none` of each repo and local `git log --since=...` / `git for-each-ref` queries. These are authoritative against the same HEAD that the API would report and do not incur API quota.
- The `GET /repos/{owner}/{repo}/stats/contributors` endpoint returned `202 Accepted` for all 4 repos across 4 retries spaced ~1.5s apart — GitHub's stats cache was cold today. A3 and H6 were computed from the local `git log` data instead of from that endpoint.
- A1 and A2 required pagination for LadybugDB (88 commits on page 1 for 30d, 292 across 3 pages for 90d) and SurrealDB (50 on page 1 for 30d, 228 across 3 pages for 90d). Page-1 caps were flagged during collection.
- A4 for SurrealDB reports "24+" because the GitHub releases API page 1 returned 57 releases with published_at ≥ 2025-04-13, but local tag analysis confirmed at least 24 tags in the last 12 months (I did not enumerate past page 1 once the threshold of 4 was long exceeded). The exact number is not load-bearing — SurrealDB blows past the ≥4 threshold either way.
- S3 (open CVEs): `https://rustsec.org/advisories/` returned zero matches for lbug / cozo / surrealdb / petgraph. GitHub Security Advisories returned 0 for LadybugDB / cozodb / petgraph and 10+ published advisories for SurrealDB (first page of the `state=published` view). I did not click through each advisory to verify patched-status because the list itself is evidence of non-zero exposure surface, and the methodology threshold is "0 or all mitigated" — "10+ published, mitigation unconfirmed on index" fails conservatively.
- S4: GitHub Search API (`/search/issues?q=repo:...+type:issue+state:X+created:>=2025-04-13`). Excludes pull requests (`type:issue`). Ratio = `open / (open + closed)`.
- S5: SurrealDB's LICENSE file was fetched directly; confirmed "Business Source License 1.1" with Change Date 2030-01-01 converting to Apache-2.0. Marked as hard fail for S5 per the instructions. Per the license-is-binary rule above, SurrealDB drops even though its weighted numeric score is 75%.
- H1: Probed `CHANGELOG.md`, `CHANGELOG.rst`, `CHANGELOG`, `HISTORY.md`, `RELEASES.md`, `CHANGES.md`, `docs/CHANGELOG.md` on both `main` and `master` branches for each repo, then verified via `git ls-tree -r HEAD --name-only | grep -i changelog` on the local clones. Only petgraph has a root-level CHANGELOG.md.
- H2: "Passing" is determined from the workflow badge SVG text (`passing` / `failing` / `no status`). LadybugDB's README has `branch=master` in its badge URL but the repo's default branch is `main` — both `branch=main` and `branch=master` badge variants return `passing`. SurrealDB's `ci.yml` badge on `main` returns `failing` at the time of this gate run (2026-04-13). CozoDB's `build.yml` badge returns `no status` because there are no completed runs on `main` since the repo went dormant (2024-12-04); the 3 most recent workflow runs are from community-fork PRs marked "Action required". Scored as 0 because "configured + passing" is the threshold — "configured but no passing run on main" does not meet it.
- H3: LadybugDB has coverage infrastructure (`.lcovrc` at root, `BUILD_LCOV` CMake option with `-fprofile-arcs -ftest-coverage` flags) but no active coverage step in any of the 18 `.github/workflows/*.yml` files and no coverage badge in the README. Scored as 0 because the methodology requires "badge OR CI step", and the infra is opt-in at build time only.
- H4 (docs.rs coverage): reads the "N out of M items documented" string from the docs.rs crate page. For LadybugDB this is the `lbug` crate on docs.rs (the actual Rust binding), not the engine repo.
- H5: `bytes / 30` per the methodology. LadybugDB is overwhelmingly C++ (7.45 MB of C++ vs 85 KB of C, zero Rust in the engine repo — the Rust binding lives in the separate `lbug` crate repo which was not clonable). 251k LOC is under the 500k threshold but the tractability caveat from the methodology applies: a solo dev debugging LadybugDB would be reading C++ and Cypher test fixtures (2.13 MB of `.cypher` files), not Rust. Reflected honestly in the number.
- H6: Computed from local `git log --since=2025-04-13 --pretty='%an'` on HEAD. Top author share = (top author's commit count) / (total commits in the window). LadybugDB: Arun Sharma 416/972 = 42.8%. SurrealDB: Stu Schwartz 137/644 = 21.3%. petgraph: Raoul Luqué 31/71 = 43.7%. Cozo: N/A (0 commits in the window → bus factor undefined; scored 0 because the threshold "< 80%" cannot be satisfied by a project with no author at all, and a dead repo's de-facto bus factor is ∞).
- petgraph is scored as the base crate per §5.3; the Cypher-subset interpreter-on-top engineering effort is evaluated at Gate 3, not here. Its weighted 60% reflects that petgraph is a healthy library crate with low release cadence and a small commit volume in the last quarter — it clears the Gate 2 bar but only just.

## Raw API response notes

- **LadybugDB crate repo discrepancy.** The `lbug` crate on crates.io (max_version 0.15.3, updated 2026-04-01) declares `repository: https://github.com/lbugdb/lbug`. Both `curl https://api.github.com/repos/lbugdb/lbug` and `curl https://github.com/lbugdb/lbug` return HTTP 404 — the `lbugdb` org exists (the engine fork lived there briefly before the 2026-01-ish rename to `LadybugDB`), but the `lbugdb/lbug` crate repo is no longer public. Per the instructions, metrics should report the worse of the two repos; since the crate repo is inaccessible, all engine-level metrics for LadybugDB are from `LadybugDB/ladybug`, and H4 (docs.rs) is from the published `lbug` crate on docs.rs (which is available independently of the source repo). This is a real discrepancy worth flagging to the decision-maker: the published crate has no inspectable source tree on GitHub, which is a supply-chain audit concern that Gate 2 does not directly score.
- **CozoDB dormancy.** HEAD commit is `2024-12-04 20:49:06 +0800` by Ziyang Hu (`zh217`), the sole original author. The last pre-dormancy merges in Nov/Dec 2024 pulled in commits from the `cozo-community/cozo` fork (visible in merge messages: "Merge pull request #290 from cozo-community/main"), which suggests the community fork is where post-2024 activity lives — but the question asked about `cozodb/cozo`, and that repo's main branch is frozen. A3 / A4 / H2 / H6 all fail as a direct consequence. `cozo-community/cozo` was not scored per the study scope (only the 4 candidates listed).
- **SurrealDB published advisories.** The `/security/advisories?state=published` page returns 10 entries on page 1 with pagination indicators suggesting more. Severities on page 1 range from Critical (SurrealQL injection) to Low (JavaScript timeout / TSV file read). I did not click through each one to verify "fixed in version X" — the index alone carries enough signal that S3 fails conservatively. If the decision-maker needs a per-advisory triage, that is a follow-up, not a Gate 2 blocker.
- **SurrealDB CI `failing` on main.** The `ci.yml` badge returns `failing` as of 2026-04-13, even though recent commits are landing daily and the repo is otherwise healthy. This could be a transient flake on a supply-chain / fuzz workflow — SurrealDB has 13 workflow files including fuzzing, crud-bench, and scorecard, any of which could be red. Scored as 0 per the literal threshold.
- **petgraph release cadence is low but acceptable.** 2 tagged releases in the last 12 months (v0.8.2 on 2025-06-06, v0.8.3 on 2025-09-30) vs threshold of 4. Commits in the last 30 and 90 days are 0 and 3 respectively — the repo enters "steady maintenance" territory, which is fine for a library crate (petgraph is mature; the large commits come in bursts around release). The 60% score reflects this honestly: petgraph passes the sanity bar but is the lowest-activity of the advancing candidates.
- **Rate limit budget.** API core quota was exhausted at ~51/60 used after ~12 min of gate work. The `git clone --bare --filter=blob:none` fallback for each repo took <90s total and unblocked all remaining metrics without needing the 26-minute reset wait. All non-API data (commit counts, authors, tag dates, tree listings) was reproduced from the local clones and is authoritative for the HEAD snapshot at 2026-04-13 ~13:00 UTC.
