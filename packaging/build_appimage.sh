#!/usr/bin/env bash
# Build a single-file AppImage for vooox.
#
# Output: dist/vooox-x86_64.AppImage
#
# Strategy: ship only the Rust binary + sidecar source. Python venv +
# faster-whisper + model are installed on first launch by the in-app setup
# wizard. System GTK4 / xdotool / python3 are expected (the wizard checks
# python and prints distro-specific install hints if anything is missing).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PKG="$ROOT/packaging"
DIST="$ROOT/dist"
APPDIR="$DIST/AppDir"
TOOL="$DIST/tools/appimagetool"
TOOL_URL="${APPIMAGETOOL_URL:-https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage}"

cd "$ROOT"

echo "==> cargo build --release"
cargo build --release --bin vooox

echo "==> fetching appimagetool (if needed)"
mkdir -p "$DIST/tools"
if [ ! -x "$TOOL" ]; then
    curl -fL --progress-bar -o "$TOOL" "$TOOL_URL"
    chmod +x "$TOOL"
fi

echo "==> assembling AppDir at $APPDIR"
rm -rf "$APPDIR"
mkdir -p \
    "$APPDIR/usr/bin" \
    "$APPDIR/usr/whisper_server" \
    "$APPDIR/usr/share/applications" \
    "$APPDIR/usr/share/icons/hicolor/scalable/apps"

cp "$ROOT/target/release/vooox" "$APPDIR/usr/bin/vooox"
strip "$APPDIR/usr/bin/vooox" 2>/dev/null || true

cp "$ROOT/whisper_server/server.py" "$APPDIR/usr/whisper_server/server.py"

install -m 0755 "$PKG/AppRun" "$APPDIR/AppRun"
install -m 0644 "$PKG/vooox.desktop" "$APPDIR/vooox.desktop"
install -m 0644 "$PKG/vooox.desktop" "$APPDIR/usr/share/applications/vooox.desktop"
install -m 0644 "$PKG/vooox.svg" "$APPDIR/vooox.svg"
install -m 0644 "$PKG/vooox.svg" "$APPDIR/usr/share/icons/hicolor/scalable/apps/vooox.svg"

# .DirIcon is what file managers actually show; appimagetool falls back to
# this when the SVG isn't rendered.
cp "$PKG/vooox.svg" "$APPDIR/.DirIcon"

echo "==> running appimagetool"
OUT="$DIST/vooox-x86_64.AppImage"
ARCH=x86_64 "$TOOL" --no-appstream "$APPDIR" "$OUT"

echo
echo "==> done"
ls -lh "$OUT"
