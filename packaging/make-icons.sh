#!/bin/sh
# Regenerates every icon file from one source image.
#
#   packaging/make-icons.sh path/to/icon.svg   (or a square PNG, 512 px or more)
#
# Writes:
#   assets/icon/neoviz-256.png    window / taskbar icon, built into the binary
#   assets/icon/neoviz.ico        Windows .exe icon (16-256 px)
#   packaging/linux/neoviz.svg    Linux app-menu icon (SVG sources only)
#   packaging/linux/neoviz-512.png  Linux app-menu icon (PNG sources only)
#
# Needs ImageMagick 7 (`magick`), plus `rsvg-convert` for SVG sources.
set -eu

src=${1:?usage: $0 icon.svg|icon.png}
root=$(cd "$(dirname "$0")/.." && pwd)
out="$root/assets/icon"
mkdir -p "$out"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

case "$src" in
*.svg)
    rsvg-convert -w 1024 -h 1024 "$src" -o "$tmp/master.png"
    cp "$src" "$root/packaging/linux/neoviz.svg"
    rm -f "$root/packaging/linux/neoviz-512.png"
    ;;
*.png)
    magick "$src" -background none -gravity center -resize 1024x1024 \
        -extent 1024x1024 "$tmp/master.png"
    magick "$tmp/master.png" -resize 512x512 "$root/packaging/linux/neoviz-512.png"
    rm -f "$root/packaging/linux/neoviz.svg"
    ;;
*)
    echo "expected an .svg or .png, got $src" >&2
    exit 1
    ;;
esac

magick "$tmp/master.png" -resize 256x256 -strip "$out/neoviz-256.png"
magick "$tmp/master.png" -define icon:auto-resize=256,128,64,48,32,24,16 "$out/neoviz.ico"
echo "icons written to $out and packaging/linux"
