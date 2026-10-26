#!/usr/bin/env bash
# Turns the device artwork in design/Icons/*.svg into asset-catalog images.
#
# The artwork is two-tone: black bodies with white linework. Each icon is
# split into a "fill" layer (the black parts) and a "line" layer (the white
# parts), both rendered as template PDFs, so the app can colour them per
# appearance: light fill and dark lines in light mode, soft blue and white
# in dark mode, or the device's own colour for the fill.
# Requires librsvg: brew install librsvg
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SRC="$ROOT/design/Icons"
OUT="$ROOT/app/Netrace/Assets.xcassets/Devices"
command -v rsvg-convert >/dev/null || { echo "error: rsvg-convert not found (brew install librsvg)" >&2; exit 1; }

rm -rf "$OUT"
mkdir -p "$OUT"
echo '{"info":{"author":"xcode","version":1}}' > "$OUT/Contents.json"

python3 - "$SRC" "$OUT" <<'PY'
import re, subprocess, sys, os, json, glob

src, out = sys.argv[1], sys.argv[2]
names = {  # file in design/Icons -> asset name
    "router": "router", "workgroup-switch": "l2Switch", "layer-3-switch": "l3Switch",
    "small-hub": "hub", "firewall": "firewall", "internet": "cloud", "pc": "pc",
    "laptop": "laptop", "file-server": "server", "printer": "printer", "phone": "ipPhone",
}
BLACK = r"(?:#000000|#000|#1a1a1a|black)\b"
WHITE = r"(?:#ffffff|#fff|white)\b"

def layer(svg, keep_black):
    # Attributes (fill="#fff") and CSS declarations (fill:#fff) alike: the
    # layer's own colour becomes black, the other layer's becomes nothing.
    def sub(m):
        prop, eq, value = m.group(1), m.group(2), m.group(3)
        is_black = re.fullmatch(BLACK, value) is not None
        colour = "#000" if is_black == keep_black else "none"
        return f"{prop}{eq}{colour}"
    pattern = rf"\b(fill|stroke)(=\"|:)({BLACK}|{WHITE})"
    return re.sub(pattern, sub, svg)

def asset(name, which, pdf_path):
    d = os.path.join(out, f"{name}.{which}.imageset")
    os.makedirs(d, exist_ok=True)
    os.replace(pdf_path, os.path.join(d, f"{name}.{which}.pdf"))
    json.dump({
        "images": [{"filename": f"{name}.{which}.pdf", "idiom": "universal"}],
        "info": {"author": "xcode", "version": 1},
        "properties": {"preserves-vector-representation": True, "template-rendering-intent": "template"},
    }, open(os.path.join(d, "Contents.json"), "w"), indent=2)

for file, name in names.items():
    svg = open(os.path.join(src, file + ".svg")).read()
    for which, keep_black in (("fill", True), ("line", False)):
        tmp = os.path.join(out, f".{name}.{which}.svg")
        open(tmp, "w").write(layer(svg, keep_black))
        pdf = os.path.join(out, f".{name}.{which}.pdf")
        subprocess.run(["rsvg-convert", "-f", "pdf", tmp, "-o", pdf], check=True)
        os.remove(tmp)
        asset(name, which, pdf)
print(f"Rendered {len(names)} device icons into {out}")
PY
