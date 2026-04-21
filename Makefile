.PHONY: test-integ-up test-integ-down release-prepare

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
