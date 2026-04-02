#!/usr/bin/env sh
# Install the `sift` binary from GitHub Releases.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/botirk38/sift/v0.1.2/scripts/install.sh | sh
# (pin the tag to the version you want, or rely on default latest-release resolution)
#
# Environment:
#   SIFT_REPO   — owner/repo (default: botirk38/sift)
#   SIFT_VERSION — release version without leading v (default: latest GitHub release)
#   PREFIX      — install directory (default: $HOME/.local)
set -eu

SIFT_REPO="${SIFT_REPO:-botirk38/sift}"
PREFIX="${PREFIX:-$HOME/.local}"
BIN_DIR="${BIN_DIR:-$PREFIX/bin}"

# Optional: pin a version, e.g. export SIFT_VERSION=0.1.2
resolve_version() {
	if [ -n "${SIFT_VERSION:-}" ]; then
		printf '%s\n' "$SIFT_VERSION"
		return
	fi
	# Latest release tag from GitHub API (strip leading v)
	_json=$(curl -fsSL "https://api.github.com/repos/${SIFT_REPO}/releases/latest") || return 1
	_ver=$(printf '%s' "$_json" | tr -d '\n' | sed 's/.*"tag_name"[[:space:]]*:[[:space:]]*"v\([^"]*\)".*/\1/')
	if [ -z "$_ver" ] || [ "$_ver" = "$_json" ]; then
		echo "install: could not resolve latest version for ${SIFT_REPO}; set SIFT_VERSION" >&2
		return 1
	fi
	printf '%s\n' "$_ver"
}

detect_asset() {
	_os=$(uname -s)
	_arch=$(uname -m)
	case "${_os}:${_arch}" in
	Linux:x86_64) printf '%s\n' "sift-x86_64-unknown-linux-gnu" ;;
	Darwin:arm64) printf '%s\n' "sift-aarch64-apple-darwin" ;;
	Darwin:x86_64) printf '%s\n' "sift-x86_64-apple-darwin" ;;
	MINGW*|MSYS*|CYGWIN*) printf '%s\n' "sift-x86_64-pc-windows-msvc.exe" ;;
	*)
		echo "install: unsupported OS/arch: ${_os} ${_arch}" >&2
		echo "install: try: cargo install --locked --git https://github.com/${SIFT_REPO}.git sift-cli" >&2
		exit 1
		;;
	esac
}

fallback_cargo() {
	if ! command -v cargo >/dev/null 2>&1; then
		echo "install: curl download failed and cargo not found; install Rust from https://rustup.rs" >&2
		exit 1
	fi
	echo "install: falling back to: cargo install --locked --git https://github.com/${SIFT_REPO}.git sift-cli" >&2
	cargo install --locked --git "https://github.com/${SIFT_REPO}.git" sift-cli
	exit 0
}

VERSION=$(resolve_version) || exit 1
ASSET=$(detect_asset)
TAG="v${VERSION}"
URL="https://github.com/${SIFT_REPO}/releases/download/${TAG}/${ASSET}"

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
