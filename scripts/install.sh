#!/usr/bin/env sh
# Install the `sift` binary from GitHub Releases.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/botirk38/sift/master/scripts/install.sh | sh
# Resolves the latest release by default (override with SIFT_VERSION=...).
#
# Environment:
#   SIFT_REPO   — owner/repo (default: botirk38/sift)
#   SIFT_VERSION — release version without leading v (default: latest GitHub release)
#   PREFIX      — install directory (default: $HOME/.local)
set -eu

SIFT_REPO="${SIFT_REPO:-botirk38/sift}"
PREFIX="${PREFIX:-$HOME/.local}"
BIN_DIR="${BIN_DIR:-$PREFIX/bin}"

# Managed by scripts/release.sh — do not edit manually.
SIFT_DEFAULT_VERSION="0.3.0"

# Optional: pin a version, e.g. export SIFT_VERSION=0.1.2
resolve_version() {
	if [ -n "${SIFT_VERSION:-}" ]; then
		printf '%s\n' "$SIFT_VERSION"
		return
	fi
	# GitHub REST API requires a User-Agent; unauthenticated calls are rate-limited (~60/hr/IP).
	_json=$(
		curl -fsSL \
			-H "Accept: application/vnd.github+json" \
			-H "User-Agent: sift-install-script" \
			"https://api.github.com/repos/${SIFT_REPO}/releases/latest"
	) || _json=""
	_ver=$(printf '%s' "$_json" | tr -d '\n' | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"v\([^"]*\)".*/\1/p' | head -1)
	if [ -n "$_ver" ]; then
		printf '%s\n' "$_ver"
		return
	fi
	echo "install: could not query releases/latest (rate limit or network); using SIFT_DEFAULT_VERSION=${SIFT_DEFAULT_VERSION} (override with SIFT_VERSION=... )" >&2
	printf '%s\n' "$SIFT_DEFAULT_VERSION"
}

detect_asset() {
	_os=$(uname -s)
	_arch=$(uname -m)
	case "${_os}:${_arch}" in
	Linux:x86_64) printf '%s\n' "sift-x86_64-unknown-linux-gnu" ;;
	Linux:aarch64) printf '%s\n' "sift-aarch64-unknown-linux-gnu" ;;
	Darwin:arm64) printf '%s\n' "sift-aarch64-apple-darwin" ;;
	Darwin:x86_64) printf '%s\n' "sift-x86_64-apple-darwin" ;;
	MINGW*|MSYS*|CYGWIN*) printf '%s\n' "sift-x86_64-pc-windows-msvc.exe" ;;
	*)
		echo "install: unsupported OS/arch: ${_os} ${_arch}" >&2
		echo "install: try: cargo install --locked --git https://github.com/${SIFT_REPO}.git sift-grep" >&2
		exit 1
		;;
	esac
}

fallback_cargo() {
	if ! command -v cargo >/dev/null 2>&1; then
		echo "install: curl download failed and cargo not found; install Rust from https://rustup.rs" >&2
		exit 1
	fi
	echo "install: falling back to: cargo install --locked --git https://github.com/${SIFT_REPO}.git sift-grep" >&2
	cargo install --locked --git "https://github.com/${SIFT_REPO}.git" sift-grep
	exit 0
}

VERSION=$(resolve_version)
ASSET=$(detect_asset)
TAG="v${VERSION}"
URL="https://github.com/${SIFT_REPO}/releases/download/${TAG}/${ASSET}"

case "$ASSET" in
*.exe) _bin_name="sift.exe" ;;
*) _bin_name="sift" ;;
esac
if [ -x "${BIN_DIR}/${_bin_name}" ]; then
	_old_ver=$("${BIN_DIR}/${_bin_name}" --version 2>/dev/null || true)
	echo "install: upgrading ${_bin_name} (${_old_ver:-unknown}) → ${TAG}" >&2
fi

TMP="${TMPDIR:-/tmp}"
DEST="${TMP}/sift-install-$$"
cleanup() {
	rm -f "$DEST"
}
trap cleanup EXIT

if ! curl -fsSL -o "$DEST" "$URL"; then
	fallback_cargo
fi

mkdir -p "$BIN_DIR"
case "$ASSET" in
*.exe)
	OUT="$BIN_DIR/sift.exe"
	mv "$DEST" "$OUT"
	;;
*)
	OUT="$BIN_DIR/sift"
	mv "$DEST" "$OUT"
	chmod +x "$OUT"
	;;
esac
trap - EXIT
rm -f "$DEST" 2>/dev/null || true

echo "Installed sift to ${OUT}"
echo "Ensure ${BIN_DIR} is on your PATH (e.g. export PATH=\"${BIN_DIR}:\$PATH\")"
