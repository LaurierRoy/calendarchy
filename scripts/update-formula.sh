#!/usr/bin/env bash
# Manual fallback: compute SHA256 hashes for a release and print formula values.
# Usage: ./scripts/update-formula.sh v0.1.0

set -euo pipefail

VERSION="${1:?Usage: $0 <tag, e.g. v0.1.0>}"
REPO="sovanesyan/calendarchy"
BASE_URL="https://github.com/${REPO}/releases/download/${VERSION}"

ARM64_URL="${BASE_URL}/calendarchy-aarch64-apple-darwin.tar.gz"
X86_64_URL="${BASE_URL}/calendarchy-x86_64-apple-darwin.tar.gz"

echo "Downloading arm64 tarball..."
ARM64_SHA=$(curl -sL "$ARM64_URL" | shasum -a 256 | cut -d' ' -f1)

echo "Downloading x86_64 tarball..."
X86_64_SHA=$(curl -sL "$X86_64_URL" | shasum -a 256 | cut -d' ' -f1)

echo ""
echo "=== Update Formula/calendarchy.rb with these values ==="
echo "version \"${VERSION#v}\""
echo "arm64 sha256: \"${ARM64_SHA}\""
echo "x86_64 sha256: \"${X86_64_SHA}\""
