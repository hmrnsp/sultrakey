#!/bin/sh
# Installs sultrakey on Linux or macOS.
#
#   Developer : curl -fsSL https://github.com/hmrnsp/sultrakey/releases/latest/download/install.sh | sh
#   Server    : curl -fsSL https://github.com/hmrnsp/sultrakey/releases/latest/download/install.sh | sudo sh
#   A given version: ... | SULTRAKEY_VERSION=0.1.0 sh
#
# This script only downloads the matching binary, checks its SHA256, then runs
# `sultrakey install`. All install logic lives in the binary itself.

set -eu

BASE="${SULTRAKEY_RELEASE_BASE:-https://github.com/hmrnsp/sultrakey/releases}"
if [ -n "${SULTRAKEY_VERSION:-}" ]; then
    URL="$BASE/download/v${SULTRAKEY_VERSION#v}"
else
    URL="$BASE/latest/download"
fi

fail() {
    printf '✗ %s\n' "$1" >&2
    if [ -n "${2:-}" ]; then
        printf 'Fix: %s\n' "$2" >&2
    fi
    exit 1
}

case "$(uname -s)" in
    Linux) os=unknown-linux-musl ;;
    Darwin) os=apple-darwin ;;
    *) fail "System $(uname -s) is not supported." "on Windows use install.ps1" ;;
esac
case "$(uname -m)" in
    x86_64 | amd64) arch=x86_64 ;;
    aarch64 | arm64) arch=aarch64 ;;
    *) fail "CPU $(uname -m) is not supported." ;;
esac
name="sultrakey-$arch-$os"

# HTTPS only, except for a test server given in SULTRAKEY_RELEASE_BASE.
case "$URL" in
    https://*) secure=1 ;;
    *) secure=0 ;;
esac
if command -v curl >/dev/null 2>&1; then
    download() {
        if [ "$secure" = 1 ]; then
            curl -fsSL --proto '=https' --tlsv1.2 -o "$2" "$1"
        else
            curl -fsSL -o "$2" "$1"
        fi
    }
elif command -v wget >/dev/null 2>&1; then
    download() { wget -q -O "$2" "$1"; }
else
    fail "curl or wget not found." "install one of them, for example: sudo yum install -y curl"
fi

if command -v sha256sum >/dev/null 2>&1; then
    sha256() { sha256sum "$1" | awk '{print $1}'; }
elif command -v shasum >/dev/null 2>&1; then
    sha256() { shasum -a 256 "$1" | awk '{print $1}'; }
else
    fail "sha256sum or shasum not found." "install coreutils"
fi

tmp=$(mktemp -d 2>/dev/null || mktemp -d -t sultrakey)
trap 'rm -rf "$tmp"' EXIT INT TERM

echo "Downloading $name ..."
download "$URL/$name" "$tmp/sultrakey" ||
    fail "Cannot download $URL/$name." "check the connection to github.com, or set HTTPS_PROXY if the server uses a proxy"
download "$URL/SHA256SUMS" "$tmp/SHA256SUMS" ||
    fail "Cannot download $URL/SHA256SUMS."

expected=$(awk -v n="$name" '{ f = $2; sub(/^\*/, "", f); if (f == n) print tolower($1) }' "$tmp/SHA256SUMS")
[ -n "$expected" ] || fail "SHA256SUMS does not list $name."
actual=$(sha256 "$tmp/sultrakey")
[ "$actual" = "$expected" ] || fail "Checksum of $name does not match; nothing was installed." "try again in a moment"

chmod +x "$tmp/sultrakey"
"$tmp/sultrakey" install
