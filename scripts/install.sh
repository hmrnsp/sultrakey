#!/bin/sh
# Pasang sultrakey di Linux atau macOS.
#
#   Developer : curl -fsSL https://github.com/hmrnsp/sultrakey/releases/latest/download/install.sh | sh
#   Server    : curl -fsSL https://github.com/hmrnsp/sultrakey/releases/latest/download/install.sh | sudo sh
#   Versi tertentu: ... | SULTRAKEY_VERSION=0.1.0 sh
#
# Skrip ini hanya mengunduh binary yang cocok, mencocokkan SHA256, lalu menjalankan
# `sultrakey install`. Semua logika pasang ada di binary itu sendiri.

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
        printf 'Solusi: %s\n' "$2" >&2
    fi
    exit 1
}

case "$(uname -s)" in
    Linux) os=unknown-linux-musl ;;
    Darwin) os=apple-darwin ;;
    *) fail "Sistem $(uname -s) tidak didukung." "di Windows pakai install.ps1" ;;
esac
case "$(uname -m)" in
    x86_64 | amd64) arch=x86_64 ;;
    aarch64 | arm64) arch=aarch64 ;;
    *) fail "CPU $(uname -m) tidak didukung." ;;
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
    fail "curl atau wget tidak ditemukan." "pasang salah satunya, contoh: sudo yum install -y curl"
fi

if command -v sha256sum >/dev/null 2>&1; then
    sha256() { sha256sum "$1" | awk '{print $1}'; }
elif command -v shasum >/dev/null 2>&1; then
    sha256() { shasum -a 256 "$1" | awk '{print $1}'; }
else
    fail "sha256sum atau shasum tidak ditemukan." "pasang coreutils"
fi

tmp=$(mktemp -d 2>/dev/null || mktemp -d -t sultrakey)
trap 'rm -rf "$tmp"' EXIT INT TERM

echo "Mengunduh $name ..."
download "$URL/$name" "$tmp/sultrakey" ||
    fail "Gagal mengunduh $URL/$name." "periksa koneksi ke github.com, atau set HTTPS_PROXY bila server memakai proxy"
download "$URL/SHA256SUMS" "$tmp/SHA256SUMS" ||
    fail "Gagal mengunduh $URL/SHA256SUMS."

expected=$(awk -v n="$name" '{ f = $2; sub(/^\*/, "", f); if (f == n) print tolower($1) }' "$tmp/SHA256SUMS")
[ -n "$expected" ] || fail "SHA256SUMS tidak memuat $name."
actual=$(sha256 "$tmp/sultrakey")
[ "$actual" = "$expected" ] || fail "Checksum $name tidak cocok; tidak ada yang dipasang." "ulangi beberapa saat lagi"

chmod +x "$tmp/sultrakey"
"$tmp/sultrakey" install
