#!/bin/bash
# Compares Classic mode with the C++ reference, pixel by pixel, on every scene.
# Usage: reference/validate.sh [time limit in seconds] [depth] [--no-kd]
cd "$(dirname "$0")/.." || exit 1
LIMIT=${1:-60}; DEPTH=${2:-3}; NO_KD=$3
KD=1; [ -n "$NO_KD" ] && KD=0
OUT=out/validation; mkdir -p "$OUT"; export FAR_TMP="$PWD/$OUT"
reference/cpp/build.sh || exit 1
cargo build --release -q -p far-cli || exit 1
FAR=target/release/far
same=0; different=0; failed=0
while IFS= read -r scene; do
  name=$(basename "$scene" .far)
  cpp="$OUT/cpp.bmp"; rs="$OUT/rs.bmp"; rm -f "$cpp" "$rs"
  c=$(DEPTH=$DEPTH perl -e "alarm $LIMIT; exec @ARGV" reference/cpp/farrt "$PWD/assets" "$scene" "$cpp" $KD 800 600 2>&1 | tail -1)
  r=$(perl -e "alarm $LIMIT; exec @ARGV" "$FAR" render "$scene" -o "$rs" --exact --depth "$DEPTH" $NO_KD 2>&1 | tail -1)
  if [ ! -f "$cpp" ] || [ ! -f "$rs" ]; then
    printf '%-20s FAILED c++=[%s] rust=[%s]\n' "$name" "${c:0:40}" "${r:0:60}"; failed=$((failed+1)); continue
  fi
  d=$("$FAR" compare "$cpp" "$rs")
  rc=${c##*rays=}; rr=${r##*rays=}
  case "$d" in different=0\ *) same=$((same+1)); status=SAME;; *) different=$((different+1)); status=DIFFERENT;; esac
  printf '%-20s %-9s rays c++=%s rust=%s | %s\n' "$name" "$status" "$rc" "$rr" "$d"
done < reference/scenes.txt
echo "--- same=$same different=$different failed=$failed"
[ "$different" -eq 0 ] && [ "$failed" -eq 0 ]
