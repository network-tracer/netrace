#!/usr/bin/env bash
# Renders design/app-icon.svg into the macOS AppIcon asset (all sizes).
# Requires librsvg: brew install librsvg
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SRC="$ROOT/design/app-icon.svg"
OUT="$ROOT/app/Netrace/Assets.xcassets/AppIcon.appiconset"
command -v rsvg-convert >/dev/null || { echo "error: rsvg-convert not found (brew install librsvg)" >&2; exit 1; }

mkdir -p "$OUT"
images=""
for size in 16 32 128 256 512; do
  for scale in 1 2; do
    px=$((size * scale))
    file="icon_${size}x${size}@${scale}x.png"
    rsvg-convert -w "$px" -h "$px" "$SRC" -o "$OUT/$file"
    images+="{\"idiom\":\"mac\",\"size\":\"${size}x${size}\",\"scale\":\"${scale}x\",\"filename\":\"$file\"},"
  done
done
printf '{"images":[%s],"info":{"author":"xcode","version":1}}\n' "${images%,}" | python3 -m json.tool > "$OUT/Contents.json"
echo "Rendered app icon into $OUT"
