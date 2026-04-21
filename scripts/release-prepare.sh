#!/usr/bin/env bash
# release-prepare.sh — Bump Cargo.toml version, generate changelog, commit.
#
# Usage:
#   scripts/release-prepare.sh [patch|minor|major|auto]
#
# Defaults to auto-detect:
#   - All commits since last tag are fix: → patch
#   - Otherwise → minor
#
# Must be run from repo root on a branch off develop.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

# ── Preflight checks ─────────────────────────────────────────────────

command -v git-cliff >/dev/null 2>&1 || { echo "FATAL: git-cliff not found. Install: cargo install git-cliff"; exit 1; }

BRANCH="$(git rev-parse --abbrev-ref HEAD)"
if [ "$BRANCH" = "main" ] || [ "$BRANCH" = "develop" ]; then
    echo "FATAL: Do not run on $BRANCH directly. Create a release branch first:"
    echo "  git checkout develop && git pull && git checkout -b release/vX.Y.Z"
    exit 1
fi

# ── Determine version ────────────────────────────────────────────────

LATEST_TAG=$(git tag -l 'v*' --sort=-v:refname | head -1)
if [ -z "$LATEST_TAG" ]; then
    echo "No existing semver tags — bootstrapping from v0.0.0"
    LATEST_TAG="v0.0.0"
fi

MAJOR=$(echo "$LATEST_TAG" | sed 's/v//' | cut -d. -f1)
MINOR=$(echo "$LATEST_TAG" | sed 's/v//' | cut -d. -f2)
PATCH=$(echo "$LATEST_TAG" | sed 's/v//' | cut -d. -f3)

BUMP="${1:-auto}"

case "$BUMP" in
    patch)
        PATCH=$((PATCH + 1))
        ;;
    minor)
        MINOR=$((MINOR + 1))
        PATCH=0
        ;;
    major)
        MAJOR=$((MAJOR + 1))
        MINOR=0
        PATCH=0
        ;;
    auto)
        if [ "$LATEST_TAG" = "v0.0.0" ]; then
            MINOR=1
            PATCH=0
        else
            COMMITS=$(git log "${LATEST_TAG}..HEAD" --no-merges --pretty=format:"%s")
            if [ -z "$COMMITS" ]; then
                echo "No new commits since $LATEST_TAG — nothing to release."
                exit 0
            fi
            if echo "$COMMITS" | grep -qvE '^fix(\(|:)'; then
                MINOR=$((MINOR + 1))
                PATCH=0
            else
                PATCH=$((PATCH + 1))
            fi
        fi
        ;;
    *)
        echo "Usage: $0 [patch|minor|major|auto]"
        exit 1
        ;;
esac

NEW_VERSION="${MAJOR}.${MINOR}.${PATCH}"
NEW_TAG="v${NEW_VERSION}"

echo "Current: $LATEST_TAG → Next: $NEW_TAG (bump: $BUMP)"
echo ""

# ── Check if tag already exists ──────────────────────────────────────

if git rev-parse "$NEW_TAG" >/dev/null 2>&1; then
    echo "FATAL: Tag $NEW_TAG already exists. Pick a different version or delete the tag."
    exit 1
fi

# ── Bump workspace version in Cargo.toml ─────────────────────────────

if ! grep -q '^version = ' Cargo.toml; then
    echo "FATAL: Could not find version line in Cargo.toml"
    exit 1
fi

sed -i "s/^version = \"[0-9]*\.[0-9]*\.[0-9]*\"/version = \"${NEW_VERSION}\"/" Cargo.toml
echo "Bumped Cargo.toml: ${NEW_VERSION}"

# ── Generate changelog ───────────────────────────────────────────────

git-cliff --tag "$NEW_TAG" -o CHANGELOG.md
echo "Generated CHANGELOG.md for $NEW_TAG"

# ── Commit ───────────────────────────────────────────────────────────

git add Cargo.toml CHANGELOG.md
git commit -m "chore(release): ${NEW_TAG}

Bump workspace version to ${NEW_VERSION} and regenerate changelog."

echo ""
echo "Done. Release commit created on branch: $BRANCH"
echo ""
echo "Next steps:"
echo "  1. Push and create PR → develop"
echo "  2. After merge to develop, create PR develop → main"
echo "  3. After merge to main, release.yml auto-tags + builds + publishes to Gitea releases"
echo "  4. Back-merge main → develop to resync"
