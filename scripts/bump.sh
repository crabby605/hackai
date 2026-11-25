#!/usr/bin/env bash
# Version bump script for HackAI
# Usage: scripts/bump.sh [fix|minor|major] [--push]

set -euo pipefail

BUMP_TYPE="${1:-}"
PUSH="${2:-}"

if [[ ! "$BUMP_TYPE" =~ ^(fix|minor|major)$ ]]; then
    echo "Usage: $0 [fix|minor|major] [--push]"
    exit 1
fi

# Safety checks
if [[ "$(git status --porcelain)" != "" ]]; then
    echo "Error: Working tree is dirty. Commit or stash changes first."
    exit 1
fi

if [[ "$(git rev-parse --abbrev-ref HEAD)" != "main" ]]; then
    echo "Error: Not on main branch."
    exit 1
fi

# Get current version from root Cargo.toml
VERSION=$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')
if [[ ! "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo "Error: Version in Cargo.toml is not plain X.Y.Z (got: $VERSION)"
    exit 1
fi

IFS='.' read -r MAJOR MINOR PATCH <<< "$VERSION"

case "$BUMP_TYPE" in
    fix)
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
esac

NEW_VERSION="${MAJOR}.${MINOR}.${PATCH}"
TAG="v${NEW_VERSION}"

# Check if tag already exists
if git rev-parse "$TAG" >/dev/null 2>&1; then
    echo "Error: Tag $TAG already exists locally."
    exit 1
fi

if git ls-remote --tags origin "$TAG" | grep -q "$TAG"; then
    echo "Error: Tag $TAG already exists on origin."
    exit 1
fi

echo "Bumping $VERSION -> $NEW_VERSION"

# Update Cargo.toml (first version = "..." line)
sed -i.bak -E "0,/^version = \".*\"/s//version = \"$NEW_VERSION\"/" Cargo.toml
rm Cargo.toml.bak

# Update Cargo.lock
cargo update --workspace

# Commit and tag
git add Cargo.toml Cargo.lock
git commit -m "chore: release $NEW_VERSION"
git tag "$TAG"

echo "Created tag $TAG locally."

if [[ "$PUSH" == "--push" ]]; then
    git push origin main
    git push origin "$TAG"
    echo "Pushed main and $TAG to origin. Release workflow triggered."
else
    echo "To trigger the release, run:"
    echo "  git push origin main"
    echo "  git push origin $TAG"
fi