#!/usr/bin/env bash
# Puts Intel Open Image Denoise into src-tauri/oidn, where the app bundle and the command line
# find it. Without it, the engine denoises with its own filter.
#
# Usage: tools/fetch_oidn.sh [macos-arm64 | macos-x86_64 | windows-x64]
# With no argument it picks this computer's system. It downloads the official release from
# GitHub, checks its SHA-256, and keeps only the CPU device and what it needs.
set -euo pipefail

VERSION=2.5.1
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TARGET="$ROOT/src-tauri/oidn"

platform=${1:-}
if [ -z "$platform" ]; then
  case "$(uname -s)-$(uname -m)" in
    Darwin-arm64) platform=macos-arm64 ;;
    Darwin-x86_64) platform=macos-x86_64 ;;
    MINGW*|MSYS*|CYGWIN*) platform=windows-x64 ;;
    *) echo "Unknown system; pass macos-arm64, macos-x86_64 or windows-x64" >&2; exit 1 ;;
  esac
fi

case "$platform" in
  macos-arm64) archive="oidn-$VERSION.arm64.macos.tar.gz"
    sha=98e0aca8e7ab69e9f4f0191582a500fd8b0d9085d68662f18ab6469e934a6efd ;;
  macos-x86_64) archive="oidn-$VERSION.x86_64.macos.tar.gz"
    sha=a4341639005a33ce0d944bd7c771ce34de1465be7eb3189b7c3b5b0064429832 ;;
  windows-x64) archive="oidn-$VERSION.x64.windows.zip"
    sha=f11f91bc072a5e3a564515724cb72ab8fcfbc445c84a84c197ff9b16cc01396f ;;
  *) echo "Unknown platform $platform" >&2; exit 1 ;;
esac

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
curl -fsSL -o "$work/$archive" "https://github.com/OpenImageDenoise/oidn/releases/download/v$VERSION/$archive"
if command -v sha256sum >/dev/null; then got=$(sha256sum "$work/$archive" | cut -d' ' -f1)
else got=$(shasum -a 256 "$work/$archive" | cut -d' ' -f1); fi
if [ "$got" != "$sha" ]; then
  echo "SHA-256 mismatch for $archive: $got" >&2; exit 1
fi

case "$archive" in
  *.zip) (cd "$work" && if command -v unzip >/dev/null; then unzip -q "$archive"; else 7z x -y "$archive" >/dev/null; fi) ;;
  *) tar -xzf "$work/$archive" -C "$work" ;;
esac
release="$work/${archive%.tar.gz}"
release="${release%.zip}"

# The libraries look for one another in ../lib, so they stay together in a folder of that name.
rm -rf "$TARGET/lib"
mkdir -p "$TARGET/lib"
case "$platform" in
  macos-*)
    # Real files under the names the libraries load each other by, so no links are needed.
    cp "$release/lib/libOpenImageDenoise.$VERSION.dylib" "$TARGET/lib/libOpenImageDenoise.2.dylib"
    cp "$release/lib/libOpenImageDenoise_core.$VERSION.dylib" "$TARGET/lib/"
    cp "$release/lib/libOpenImageDenoise_device_cpu.$VERSION.dylib" "$TARGET/lib/"
    cp "$release/lib/libtbb.12.19.dylib" "$TARGET/lib/libtbb.12.dylib"
    ;;
  windows-x64)
    for f in OpenImageDenoise.dll OpenImageDenoise_core.dll OpenImageDenoise_device_cpu.dll tbb12.dll; do
      cp "$release/bin/$f" "$TARGET/lib/"
    done
    ;;
esac
mkdir -p "$TARGET/licenses"
cp "$release/doc/LICENSE.txt" "$TARGET/licenses/OIDN-LICENSE.txt"
cp "$release/doc/third-party-programs-oneTBB.txt" "$TARGET/licenses/"
echo "Open Image Denoise $VERSION ($platform) in $TARGET"
