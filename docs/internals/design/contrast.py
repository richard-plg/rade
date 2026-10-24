#!/usr/bin/env python3
"""WCAG 2.2 contrast of the RADE design tokens (PRD R102.2, ADR-0036 §3).

Reads crates/rade-app/src/tokens.toml (the only place a color is written) and
checks every pair of its [contrast] table in both themes: `text` pairs need
4.5 : 1, `glyph` pairs (icons, LEDs) 3 : 1. A pair is
"<ink> on <background>… [over <base>]"; a translucent background is composited
over its base, a translucent ink over the result.

Usage: python3 docs/internals/design/contrast.py [tokens.toml]
Exit 0 when every line says OK, 1 when one is LOW (the line names the pair),
2 on a malformed file.
"""
import os
import sys
import tomllib

HERE = os.path.dirname(os.path.abspath(__file__))
DEFAULT = os.path.join(HERE, "..", "..", "crates", "rade-app", "src", "tokens.toml")
NEED = {"text": 4.5, "glyph": 3.0}


def rgba(h):
    h = h.lstrip("#")
    if len(h) not in (6, 8):
        raise ValueError(f"not #rrggbb[aa]: #{h}")
    v = [int(h[i:i + 2], 16) for i in range(0, len(h), 2)]
    return v[:3], (v[3] / 255 if len(v) == 4 else 1.0)


def over(top, base):
    """`top` (#hex, maybe translucent) composited over the opaque rgb `base`."""
    c, a = rgba(top)
    return [round(a * x + (1 - a) * y) for x, y in zip(c, base)]


def lum(c):
    f = lambda x: (x / 255) / 12.92 if x / 255 <= 0.04045 else ((x / 255 + 0.055) / 1.055) ** 2.4
    r, g, b = map(f, c)
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


def ratio(a, b):
    la, lb = sorted([lum(a), lum(b)], reverse=True)
    return (la + 0.05) / (lb + 0.05)


def pairs(spec):
    """"fg on bg0 bg1 [over bg2]" → [(ink, bg, base or None), …]"""
    words = spec.split()
    if len(words) < 3 or words[1] != "on":
        raise ValueError(f"bad pair '{spec}' (want '<ink> on <bg>… [over <base>]')")
    ink, rest, base = words[0], words[2:], None
    if "over" in rest:
        i = rest.index("over")
        if i == 0 or len(rest) != i + 2:
            raise ValueError(f"bad pair '{spec}'")
        rest, base = rest[:i], rest[i + 1]
    return [(ink, bg, base) for bg in rest]


def main(path):
    try:
        with open(path, "rb") as f:
            data = tomllib.load(f)
        themes = {t: data[t] for t in ("dark", "light")}
        table = data["contrast"]
    except (OSError, KeyError, tomllib.TOMLDecodeError) as e:
        print(f"contrast.py: {path}: {e}", file=sys.stderr)
        return 2
    ok = True
    try:
        for theme, tok in themes.items():
            for kind in ("text", "glyph"):
                for spec in table.get(kind, []):
                    for ink, bg, base in pairs(spec):
                        for n in (ink, bg, base):
                            if n is not None and n not in tok:
                                raise ValueError(f"[{theme}] has no token '{n}' (pair '{spec}')")
                        if base is None and rgba(tok[bg])[1] < 1:
                            raise ValueError(f"translucent '{bg}' needs 'over <base>' (pair '{spec}')")
                        under = rgba(tok[base])[0] if base else [0, 0, 0]
                        b = over(tok[bg], under)
                        r = ratio(over(tok[ink], b), b)
                        need = NEED[kind]
                        name = f"{theme} {ink} on {bg}" + (f" over {base}" if base else "")
                        print(f"{'OK ' if r >= need else 'LOW'} {r:5.2f} (need {need}) {name}")
                        ok &= r >= need
    except ValueError as e:
        print(f"contrast.py: {path}: {e}", file=sys.stderr)
        return 2
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1] if len(sys.argv) > 1 else DEFAULT))
