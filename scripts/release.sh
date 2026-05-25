#!/usr/bin/env bash
# Cut a new sift release.
#
# Usage: ./scripts/release.sh <version>
#   version — semver without leading v (e.g. 0.2.0)
#
# What it does:
#   1. Validates the version string
#   2. Updates workspace version in Cargo.toml
#   3. Regenerates Cargo.lock
#   4. Updates SIFT_DEFAULT_VERSION in scripts/install.sh
#   5. Updates the install URL in README.md
#   6. Generates/prepends changelog via git-cliff
#   7. Commits everything as "release: v<version>"
#   8. Creates a git tag v<version>
#   9. Prints push instructions
set -euo pipefail

if [ $# -ne 1 ]; then
	echo "Usage: $0 <version>" >&2
	echo "  e.g. $0 0.2.0" >&2
	exit 1
fi

VERSION="$1"
TAG="v${VERSION}"

# Validate semver format.
if ! printf '%s' "$VERSION" | grep -qE '^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.]+)?$'; then
	echo "Error: '$VERSION' is not a valid semver version" >&2
	exit 1
fi

# Ensure we're on a clean working tree.
if [ -n "$(git status --porcelain)" ]; then
	echo "Error: working tree is dirty — commit or stash changes first" >&2
	exit 1
fi

# Ensure the tag doesn't already exist.
if git rev-parse "$TAG" >/dev/null 2>&1; then
	echo "Error: tag '$TAG' already exists" >&2
	exit 1
fi

# Require git-cliff.
if ! command -v git-cliff >/dev/null 2>&1; then
	echo "Error: git-cliff not found — install with: cargo install git-cliff" >&2
	exit 1
fi

echo "Releasing ${TAG}..."

# 1. Bump workspace version in Cargo.toml.
sed -i "s/^version = \".*\"/version = \"${VERSION}\"/" Cargo.toml

# 2. Regenerate Cargo.lock.
cargo check --workspace --quiet 2>/dev/null || cargo check --workspace

# 3. Update install.sh fallback version.
sed -i "s/SIFT_DEFAULT_VERSION=\"[^\"]*\"/SIFT_DEFAULT_VERSION=\"${VERSION}\"/" scripts/install.sh

# 4. Update README.md install URL.
sed -i "s|/sift/v[0-9][0-9.]*[0-9]/scripts/install.sh|/sift/${TAG}/scripts/install.sh|" README.md

# 5. Generate changelog (prepend new version, keep history).
git-cliff --config cliff.toml --tag "$TAG" -o CHANGELOG.md

# 6. Commit and tag.
git add Cargo.toml Cargo.lock scripts/install.sh README.md CHANGELOG.md
git commit -m "release: ${TAG}"
git tag -a "$TAG" -m "release: ${TAG}"

echo ""
echo "Done. Review the commit, then push:"
echo "  git push origin master --follow-tags"
