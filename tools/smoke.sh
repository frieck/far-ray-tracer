#!/usr/bin/env bash
# Renders every example scene small and denoised with the release command line, as a quick
# check that a build works on this system. Fails if a scene does not render, or if Open Image
# Denoise (fetched by tools/fetch_oidn.sh into src-tauri/oidn) does not load.
#
# Usage: tools/smoke.sh        Used by .github/workflows/ci.yml; runs locally too.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
cargo build --release -p far-cli
FAR="$ROOT/target/release/far"
[ -x "$FAR.exe" ] && FAR="$FAR.exe"
OUT="$(mktemp -d)"
trap 'rm -rf "$OUT"' EXIT

failed=0
count=0
while IFS= read -r scene; do
  count=$((count + 1))
  name="$(basename "$scene" .far)"
  if ! log="$("$FAR" render "$scene" -o "$OUT/$name.png" --enhanced --samples 4 --width 96 --height 72 --denoise 2>&1)"; then
    echo "::error::$scene did not render"
    echo "$log"
    failed=1
    continue
  fi
  if ! grep -q "denoiser=oidn" <<< "$log"; then
    echo "::error::$scene was not denoised by Open Image Denoise"
    echo "$log"
    failed=1
    continue
  fi
  echo "ok  $scene"
done < <(find assets/scenes -name '*.far' | sort)

echo "$count scenes"
exit $failed
