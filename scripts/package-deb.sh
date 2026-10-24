#!/usr/bin/env bash
# Build target/debian/rade_<version>_amd64.deb (M10, ADR-0008) with plain
# dpkg-deb: no cargo-deb needed. Layout:
#   /usr/bin/rade                      CLI
#   /usr/bin/rade-app -> ../lib/rade/rade-app
#   /usr/lib/rade/{rade-app, libghostty-internal.so, rade -> ../../bin/rade}
#                                      (RUNPATH $ORIGIN; `rade` next to the app
#                                      because Panes get the app's dir on PATH)
#   /usr/share/rade/{ghostty/{shell-integration,themes}, terminfo}
#   /usr/share/rade/{skills/rade/SKILL.md, skills/rade-workspace/SKILL.md, AGENTS-snippet.md}
#   /usr/share/applications/dev.rade.App.desktop, hicolor icons, doc
# The release build goes to target/deb-build so target/release (dev builds,
# tests, the owner's pre-package launcher) is never relinked.
set -euo pipefail
umask 022
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
# Tagged release → <version>; else <version>+git<date>.<sha>, so each commit
# sorts higher and `apt install ./…deb` upgrades (scripts/version.sh, ADR-0012).
# CI builds without .git and passes RADE_VERSION.
VERSION=${RADE_VERSION:-$(scripts/version.sh)}
MAINTAINER="Richard <richard.plg202@gmail.com>"
ARCH=$(dpkg --print-architecture)
GLIB=ghostty/zig-out/lib/libghostty-internal.so
GSHARE=ghostty/zig-out/share

# Always (zig caches): a stale zig-out after a submodule bump must not ship.
echo "building libghostty (ReleaseFast)…"
(cd ghostty && zig build -Dapp-runtime=none -Doptimize=ReleaseFast)

export CARGO_TARGET_DIR=$ROOT/target/deb-build
RADE_LIBGHOSTTY_RPATH='$ORIGIN' cargo build --release --locked -p rade-app -p rade-cli
BIN=$CARGO_TARGET_DIR/release

WORK=$(mktemp -d "$ROOT/target/deb-work.XXXXXX")
trap 'rm -rf "$WORK"' EXIT
PKG=$WORK/debian/rade # dpkg-shlibdeps treats files under debian/<pkg> as the package
install -d "$PKG"/DEBIAN "$PKG"/usr/bin "$PKG"/usr/lib/rade "$PKG"/usr/share/rade/skills/rade \
    "$PKG"/usr/share/applications "$PKG"/usr/share/doc/rade "$PKG"/usr/share/icons/hicolor/scalable/apps

install -m755 -s "$BIN/rade" "$PKG/usr/bin/rade"
install -m755 -s "$BIN/rade-app" "$PKG/usr/lib/rade/rade-app"
install -m644 "$GLIB" "$PKG/usr/lib/rade/libghostty-internal.so"
# Zig links it with RUNPATH /usr/lib/<triplet>, a default search path anyway
# (lintian custom-library-search-path). The CI image has patchelf, so
# released packages never carry it; a local build without it only warns.
if command -v patchelf >/dev/null; then
    patchelf --remove-rpath "$PKG/usr/lib/rade/libghostty-internal.so"
    ! readelf -d "$PKG/usr/lib/rade/libghostty-internal.so" | grep -qE 'RUNPATH|RPATH' \
        || { echo "libghostty-internal.so still has a RUNPATH" >&2; exit 1; }
else
    echo "warning: no patchelf, libghostty-internal.so keeps its RUNPATH (apt install patchelf)" >&2
fi
ln -s ../lib/rade/rade-app "$PKG/usr/bin/rade-app"
ln -s ../../bin/rade "$PKG/usr/lib/rade/rade"
[[ $(readelf -d "$PKG/usr/lib/rade/rade-app" | grep -o 'RUNPATH.*') == *'[$ORIGIN]'* ]] \
    || { echo "rade-app RUNPATH is not \$ORIGIN" >&2; exit 1; }

# Ghostty runtime resources, same version as the bundled lib (found by
# terminal/runtime_env.rs at <prefix>/share/rade/ghostty; terminfo beside it).
install -d "$PKG/usr/share/rade/ghostty"
cp -r "$GSHARE/ghostty/shell-integration" "$GSHARE/ghostty/themes" "$PKG/usr/share/rade/ghostty/"
cp -r "$GSHARE/terminfo" "$PKG/usr/share/rade/terminfo"

install -m644 skills/rade/SKILL.md "$PKG/usr/share/rade/skills/rade/SKILL.md"
install -Dm644 skills/rade-workspace/SKILL.md "$PKG/usr/share/rade/skills/rade-workspace/SKILL.md"
install -m644 docs/AGENTS-snippet.md "$PKG/usr/share/rade/AGENTS-snippet.md"
install -m644 packaging/dev.rade.App.desktop "$PKG/usr/share/applications/"
install -m644 packaging/dev.rade.App.svg "$PKG/usr/share/icons/hicolor/scalable/apps/"
for s in 16 32 48 64 128 256 512; do
    d=$PKG/usr/share/icons/hicolor/${s}x$s/apps
    install -d "$d"
    python3 - "$s" "$d/dev.rade.App.png" <<'PY'
import sys, gi
gi.require_version("GdkPixbuf", "2.0")
from gi.repository import GdkPixbuf
s = int(sys.argv[1])
GdkPixbuf.Pixbuf.new_from_file_at_size("packaging/dev.rade.App.svg", s, s).savev(sys.argv[2], "png", [], [])
PY
done
install -m644 packaging/copyright THIRD_PARTY.md crates/rade-app/assets/mermaid.LICENSE \
    crates/rade-app/assets/fonts/Inter-OFL.txt crates/rade-app/assets/fonts/JetBrainsMono-OFL.txt "$PKG/usr/share/doc/rade/"
# License texts of the Rust crates linked into the binaries (normal deps).
cargo tree -e normal -p rade-app -p rade-cli --prefix none --format '{p}' | sort -u \
    | python3 -c '
import json, pathlib, subprocess, sys
want = {tuple(l.split()[:2]) for l in sys.stdin if l.strip()}
meta = json.loads(subprocess.check_output(["cargo", "metadata", "--format-version", "1"]))
for p in sorted(meta["packages"], key=lambda p: p["name"]):
    if (p["name"], "v" + p["version"]) not in want or p["source"] is None:
        continue
    d = pathlib.Path(p["manifest_path"]).parent
    print(f"=== {p["name"]} {p["version"]} ({p.get("license") or "see files"})\n")
    for f in sorted(d.glob("LICEN[CS]E*")) + sorted(d.glob("COPYING*")):
        if f.is_file():
            print(f.read_text(errors="replace").strip() + "\n")
' | gzip -9n > "$PKG/usr/share/doc/rade/rust-crates-licenses.txt.gz"
install -m644 CHANGELOG.md "$PKG/usr/share/doc/rade/"
printf 'rade (%s) unstable; urgency=medium\n\n  * See /usr/share/doc/rade/CHANGELOG.md.\n\n -- %s  %s\n' \
    "$VERSION" "$MAINTAINER" "$(date -R ${SOURCE_DATE_EPOCH:+-d @$SOURCE_DATE_EPOCH})" | gzip -9n > "$PKG/usr/share/doc/rade/changelog.gz"
find "$PKG/usr/share" -type d -exec chmod 755 {} + && find "$PKG/usr/share" -type f -exec chmod 644 {} +

# Depends from the ELF NEEDED entries. libGL is dlopen()ed by GLAD (not
# NEEDED), so it is added by hand.
printf 'Source: rade\n\nPackage: rade\nArchitecture: any\n' > "$WORK/debian/control"
DEPS=$(cd "$WORK" && dpkg-shlibdeps -O -l"$PKG/usr/lib/rade" \
    debian/rade/usr/bin/rade debian/rade/usr/lib/rade/rade-app debian/rade/usr/lib/rade/libghostty-internal.so 2>"$WORK/shlibdeps.err" \
    | sed -n 's/^shlibs:Depends=//p') || { cat "$WORK/shlibdeps.err" >&2; exit 1; }
# Known noise: usrmerge diversions, the unversioned private libghostty.
grep -Ev 'diversion|libghostty-internal' "$WORK/shlibdeps.err" >&2 || true
SIZE=$(du -sk --exclude=DEBIAN "$PKG" | cut -f1)

cat > "$PKG/DEBIAN/control" <<CTL
Package: rade
Version: $VERSION
Section: devel
Priority: optional
Architecture: $ARCH
Maintainer: $MAINTAINER
Installed-Size: $SIZE
Depends: ${DEPS:+$DEPS, }libgl1
Recommends: git
Description: Agentic Development Environment (GTK4 + Ghostty)
 RADE runs coding agents (Claude Code, Codex, Antigravity, OpenCode) side by
 side in GPU terminal panes (embedded Ghostty), with a rade CLI that lets
 any agent spawn and drive other agents, worktrees, review and fan-out.
CTL

mkdir -p target/debian && rm -f target/debian/rade_*.deb # one .deb: `apt install ./target/debian/rade_*.deb`
OUTDEB=target/debian/rade_${VERSION}_$ARCH.deb
dpkg-deb --root-owner-group -Zxz --build "$PKG" "$OUTDEB" >/dev/null
echo "$OUTDEB ($(du -h "$OUTDEB" | cut -f1))"
