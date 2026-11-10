#!/usr/bin/env bash
# Regenerates every image in examples/. Run from the repository root.
set -euo pipefail

stitchr="${STITCHR:-stitchr}"
light=docs/examples/light.png
dark=docs/examples/dark.png
overlay=docs/examples/overlay.png
out=examples

rm -f "$out"/*.png "$out"/*.jpg

"$stitchr" "$light" "$dark" --layout horizontal \
  -o "$out/01-horizontal.png"

"$stitchr" "$light" "$dark" --layout vertical \
  -o "$out/02-vertical.png"

"$stitchr" "$light" "$dark" "$dark" "$light" --layout grid --columns 2 \
  -o "$out/03-grid.png"

"$stitchr" "$light" "$dark" --layout freeform --position 0 0 --position 240 120 \
  -o "$out/04-freeform.png"

"$stitchr" "$light" "$dark" --layout split \
  -o "$out/05-split.png"

"$stitchr" "$light" "$dark" --layout horizontal --spacing 24 \
  -o "$out/06-spacing.png"

"$stitchr" "$overlay" "$light" "$overlay" --layout horizontal --spacing 16 --alignment center \
  -o "$out/07-alignment.png"

"$stitchr" "$light" "$dark" --layout split --divider 6 \
  -o "$out/08-split-divider.png"

"$stitchr" "$light" "$dark" --layout horizontal --spacing 24 --background '#14b8a6' \
  -o "$out/09-background-color.png"

"$stitchr" "$light" "$dark" --layout horizontal --spacing 40 \
  --background-gradient '#ff7e5f' '#6a3093' --gradient-angle 135 \
  -o "$out/10-linear-gradient.png"

"$stitchr" "$light" "$dark" --layout vertical --spacing 40 \
  --background-gradient '#e0eafc' '#1e3c72' --gradient-kind radial \
  -o "$out/11-radial-gradient.png"

"$stitchr" "$light" "$dark" --layout horizontal --spacing 24 --background '#e2e8f0' \
  --label "" --label "" --border-width 4 --border-color '#334155' \
  -o "$out/12-borders.png"

"$stitchr" "$light" "$dark" --layout horizontal --spacing 24 --background '#e2e8f0' \
  --label Light --label Dark \
  -o "$out/13-labels.png"

"$stitchr" "$light" "$dark" --layout horizontal --spacing 24 --background '#e2e8f0' \
  --label Before --label After --label-size 32 --border-width 4 --border-color '#0ea5e9' \
  -o "$out/14-label-style.png"

"$stitchr" "$light" "$dark" --layout split --divider 4 --background white \
  --label Light --label Dark --border-color '#1e293b' \
  -o "$out/15-labeled-split.png"

"$stitchr" "$dark" --spacing 48 \
  --background-gradient '#4facfe' '#6a3093' --gradient-angle 135 \
  --label "Dark theme" --label-size 24 --border-width 3 --border-color white \
  -o "$out/16-single-image.png"

"$stitchr" "$light" "$dark" --layout horizontal --spacing 40 \
  --background-gradient '#ff7e5f' '#6a3093' --gradient-angle 135 \
  --label Light --label Dark --border-color white --border-width 3 \
  -o "$out/17-before-after.png"

"$stitchr" "$light" "$dark" "$dark" "$light" --layout grid --columns 2 --spacing 32 \
  --background-gradient '#4facfe' '#6a3093' --gradient-angle 120 \
  --label "Light · v1" --label "Dark · v1" --label "Dark · v2" --label "Light · v2" \
  --label-size 22 --border-width 3 --border-color white \
  -o "$out/18-labeled-grid.png"

"$stitchr" "$light" "$dark" --layout horizontal --spacing 24 --background '#14b8a6' \
  -o "$out/19-jpeg.jpg"
