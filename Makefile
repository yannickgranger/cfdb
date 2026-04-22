.PHONY: test-integ-up test-integ-down release-prepare graph-specs-check

# cfdb is a pure-library workspace. It has no Podman-backed integration
# infrastructure — every test is either a unit test, a dogfood test (cfdb
# running on its own tree), or a cross-dogfood test (cfdb vs
# graph-specs-rust at a pinned SHA). These targets exist to satisfy
# quality-preflight's makefile-integ-targets contract and are intentional
# no-ops. See CLAUDE.md §3 for the full dogfood gate catalog.

test-integ-up:
	@echo "cfdb: no Podman integ infrastructure required (pure-library workspace)"

test-integ-down:
	@echo "cfdb: no Podman integ infrastructure required (pure-library workspace)"

release-prepare:  ## Bump version + changelog (auto|patch|minor|major)
	scripts/release-prepare.sh $(or $(BUMP),auto)

# Two-pass graph-specs anti-drift gate. Mirrors `.gitea/workflows/ci.yml`
# verbatim: --code is single-value, so concepts/ and tools/ are gated
# independently against their disjoint code roots. spikes/ is structurally
# excluded (not walked by either pass — the pattern documented in #137).
# Used by /ship Step 2.7 to fail fast on spec drift before quality-ship
# burns compile time. RFC-030 §3.1 anti-drift gate.
graph-specs-check:
	@command -v graph-specs >/dev/null 2>&1 || { \
		echo "graph-specs not installed. Install: cargo install --git https://agency.lab:3000/yg/graph-specs-rust --branch develop --bin graph-specs application"; \
		exit 1; \
	}
	graph-specs check --specs specs/concepts/ --code crates/
	graph-specs check --specs specs/tools/ --code tools/
