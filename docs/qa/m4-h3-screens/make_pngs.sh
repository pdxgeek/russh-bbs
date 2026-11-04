#!/bin/bash
# Render every capture to a PNG, and the truecolor designs from the test harness.
#   docs/qa/m4-h3-screens/make_pngs.sh
# The captures (`ssh-*/NAME.cells`) come from `walk.py`; the designs are every screen the tests
# draw, dumped at the sizes the owner named, with the theme's roles and truecolor accents
# (the board composites doors at sixteen colours today, D104; these show how the same screens look
# once the member setting has a depth).
set -e
cd "$(dirname "$0")"
for dir in ssh-*; do
  for cells in "$dir"/*.cells; do
    python3 render.py "$cells" "${cells%.cells}.png"
  done
done
root=$(git rev-parse --show-toplevel)
for size in 132x43 100x30 74x20; do
  out="design-$size"
  rm -rf "$out"
  HALLPASS_DUMP="$PWD/$out" HALLPASS_SIZE=$size cargo test -q -p russh-bbs-door-hallpass --test ui show --manifest-path "$root/Cargo.toml" -- --exact >/dev/null
  for cells in "$out"/*.cells; do
    name=$(basename "$cells" .cells)
    python3 render.py "$cells" "$out/${name%%-$size-*}.png"
    rm "$cells"
  done
done
