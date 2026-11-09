#!/usr/bin/env bash
# Renders the example scenes shown in the README into docs/images, as JPEG.
#
# Usage: tools/screenshots.sh [scene names...]   (default: all of them)
# Needs the release command line (cargo build --release -p far-cli) and, for JPEG, macOS's
# sips; elsewhere the PNG is kept. Takes several minutes: smoke and fog are slow.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/docs/images"
FAR="$ROOT/target/release/far"
mkdir -p "$OUT"

# name, scene, samples, extra options
SHOTS=(
  "showcase|showcase.far|256|--caustics --bloom"
  "glass|materials/glass.far|256|--caustics"
  "coatings|materials/coatings.far|256|"
  "subsurface|materials/subsurface.far|256|"
  "spotlights|techniques/spotlights.far|256|"
  "sun-and-sky|techniques/sun-and-sky.far|256|"
  "smoke|techniques/smoke.far|192|"
  "light-shafts|techniques/light-shafts.far|192|"
  "bokeh|techniques/bokeh.far|768|"
  "models|techniques/models.far|256|"
)

wanted=("$@")
for shot in "${SHOTS[@]}"; do
  IFS='|' read -r name scene samples extra <<< "$shot"
  if [ ${#wanted[@]} -gt 0 ] && [[ ! " ${wanted[*]} " =~ " $name " ]]; then continue; fi
  png="$OUT/$name.png"
  # shellcheck disable=SC2086
  "$FAR" render "$ROOT/assets/scenes/$scene" -o "$png" --enhanced --samples "$samples" --width 960 --height 600 \
    --denoise --tone-mapping agx-punchy $extra > /dev/null
  if command -v sips > /dev/null; then
    sips -s format jpeg -s formatOptions 85 "$png" --out "$OUT/$name.jpg" > /dev/null && rm "$png"
  fi
  echo "$name"
done
