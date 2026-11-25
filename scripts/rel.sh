#!/usr/bin/env bash
# Usage: scripts/bump.sh <major|minor|fix> [--push]
#
#   major  X.y.z -> (X+1).0.0
#   minor  x.Y.z -> x.(Y+1).0
#   fix    x.y.Z -> x.y.(Z+1)
#
# Bumps the first `version = "..."` in Cargo.toml, refreshes Cargo.lock,
# commits, and tags vX.Y.Z. Pushes main + tag only with --push
# (the tag push is what triggers the release workflow).
set -euo pipefail

KIND="${1:-}"
PUSH="${2:-}"

if [[ ! "$KIND" =~ ^(major|minor|fix)$ ]]; then
  echo "usage: $0 <major|minor|fix> [--push]" >&2
  exit 1
fi
if [[ -n "$PUSH" && "$PUSH" != "--push" ]]; then
  echo "unknown flag: $PUSH" >&2
  exit 1
fi

cd "$(git rev-parse --show-toplevel)"

if [[ -n "$(git status --porcelain)" ]]; then
  echo "working tree is dirty, commit or stash first" >&2
  exit 1
fi

BRANCH="$(git rev-parse --abbrev-ref HEAD)"
if [[ "$BRANCH" != "main" ]]; then
  echo "on '$BRANCH', not main. Switch to main first." >&2
  exit 1
fi

CURRENT="$(grep -m1 -E '^version[[:space:]]*=' Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')"
if [[ ! "$CURRENT" =~ ^([0-9]+)\.([0-9]+)\.([0-9]+)$ ]]; then
  echo "can't parse plain X.Y.Z from Cargo.toml (got '$CURRENT'). Prereleases need a manual bump." >&2
  exit 1
fi
MAJOR="${BASH_REMATCH[1]}"
MINOR="${BASH_REMATCH[2]}"
FIX="${BASH_REMATCH[3]}"

case "$KIND" in
  major) MAJOR=$((MAJOR + 1)); MINOR=0; FIX=0 ;;
  minor) MINOR=$((MINOR + 1)); FIX=0 ;;
  fix)   FIX=$((FIX + 1)) ;;
esac

NEW="$MAJOR.$MINOR.$FIX"
TAG="v$NEW"

if git rev-parse -q --verify "refs/tags/$TAG" >/dev/null \
   || git ls-remote --exit-code --tags origin "$TAG" >/dev/null 2>&1; then
  echo "tag $TAG already exists" >&2
  exit 1
fi

echo "$CURRENT -> $NEW ($TAG)"

# Rewrite only the first version line (portable across GNU/BSD).
awk -v new="$NEW" '
  !done && /^version[[:space:]]*=/ { sub(/"[^"]*"/, "\"" new "\""); done = 1 }
  { print }
' Cargo.toml > Cargo.toml.tmp && mv Cargo.toml.tmp Cargo.toml

# Refresh Cargo.lock without building the whole tree.
cargo update --workspace >/dev/null 2>&1 || cargo check >/dev/null

git add Cargo.toml Cargo.lock
git commit -m "release $TAG"
git tag "$TAG"

if [[ "$PUSH" == "--push" ]]; then
  git push origin main "$TAG"
  echo "pushed $TAG, release workflow should be running:"
  echo "  gh run list --workflow=release.yml --limit 1"
else
  echo "done locally. To release:"
  echo "  git push origin main $TAG"
fi
