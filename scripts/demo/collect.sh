#!/usr/bin/env bash
# Runs every demo scene (unless NO_RUN=1), then copies each image the docs
# reference (docs/**/*.md, README.md: ../images/<file> or docs/images/<file>)
# from target/demo/*/ into docs/images/ (PNGs quantized to 256 colours), and
# removes images nothing references. Exit 1 when a referenced image was not produced.
set -uo pipefail
ROOT=$(cd "$(dirname "$0")/../.." && pwd)
cd "$ROOT"
if [[ ${NO_RUN:-} != 1 ]]; then
    [[ ${NO_BUILD:-} == 1 ]] || cargo build --release -q || exit 1
    for s in scripts/demo/*.sh; do
        case ${s##*/} in lib.sh|world.sh|agent.sh|devserver.sh|collect.sh) continue ;; esac
        echo "== ${s##*/}"
        NO_BUILD=1 "$s" >"target/demo-${s##*/}.log" 2>&1 || echo "   failed (target/demo-${s##*/}.log)"
    done
fi
mkdir -p docs/images
mapfile -t want < <(grep -rhoE '(\.\./)*(docs/)?images/[A-Za-z0-9._-]+\.(png|gif|mp4)' README.md docs \
    | sed -E 's#.*images/##' | sort -u)
missing=0
for f in "${want[@]}"; do
    src=$(ls -t target/demo/*/"$f" 2>/dev/null | head -1)
    if [[ -z $src ]]; then echo "missing: $f"; missing=1; continue; fi
    cp "$src" "docs/images/$f"
done
# Screenshots → 256-colour PNGs: a third of the size, no visible loss on UI text.
python3 -I - docs/images/*.png <<'PY'
import sys
from PIL import Image
for f in sys.argv[1:]:
    im = Image.open(f)
    if im.mode == "P":
        continue
    im.convert("RGB").quantize(colors=256, method=Image.Quantize.MEDIANCUT,
                               dither=Image.Dither.NONE).save(f, optimize=True)
PY
for f in docs/images/*; do
    [[ -e $f ]] || continue
    printf '%s\n' "${want[@]}" | grep -qxF "${f##*/}" || { echo "unused, removed: ${f##*/}"; rm "$f"; }
done
echo "docs/images: ${#want[@]} files, $(du -sh docs/images | cut -f1)"
exit $missing
