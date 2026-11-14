#!/usr/bin/env sh
# Installs neoViz for the current user (no root needed):
#   binary  -> ~/.local/bin/neoviz
#   menu    -> ~/.local/share/applications/neoviz.desktop
#   icon    -> ~/.local/share/icons/hicolor/{scalable,512x512}/apps/neoviz.{svg,png}
#
# Usage:
#   ./install.sh [path/to/neoviz]   install (default: the binary next to this
#                                   script, or target/dist/neoviz in the repo)
#   ./install.sh --uninstall        remove everything installed above
set -eu

here="$(cd "$(dirname "$0")" && pwd)"
bin_dir="$HOME/.local/bin"
data_dir="${XDG_DATA_HOME:-$HOME/.local/share}"
desktop="$data_dir/applications/neoviz.desktop"
icon_svg="$data_dir/icons/hicolor/scalable/apps/neoviz.svg"
icon_png="$data_dir/icons/hicolor/512x512/apps/neoviz.png"

refresh_menu() {
    command -v update-desktop-database >/dev/null 2>&1 \
        && update-desktop-database "$data_dir/applications" >/dev/null 2>&1 || true
    command -v gtk-update-icon-cache >/dev/null 2>&1 \
        && gtk-update-icon-cache -q "$data_dir/icons/hicolor" >/dev/null 2>&1 || true
    command -v kbuildsycoca6 >/dev/null 2>&1 && kbuildsycoca6 >/dev/null 2>&1 || true
}

if [ "${1:-}" = "--uninstall" ]; then
    rm -f "$bin_dir/neoviz" "$desktop" "$icon_svg" "$icon_png"
    refresh_menu
    echo "neoViz uninstalled. (Settings in ~/.config/neoviz were left in place.)"
    exit 0
fi

if [ $# -ge 1 ]; then
    binary="$1"
elif [ -x "$here/neoviz" ]; then
    binary="$here/neoviz"
elif [ -x "$here/../../target/dist/neoviz" ]; then
    binary="$here/../../target/dist/neoviz"
else
    echo "No neoviz binary found. Build one first: cargo build --profile dist" >&2
    exit 1
fi

install -Dm755 "$binary" "$bin_dir/neoviz"
# The icon ships as an SVG or a 512 px PNG (see packaging/make-icons.sh).
rm -f "$icon_svg" "$icon_png"
if [ -f "$here/neoviz.svg" ]; then
    install -Dm644 "$here/neoviz.svg" "$icon_svg"
else
    install -Dm644 "$here/neoviz-512.png" "$icon_png"
fi
# Point the menu entry at the installed binary, so it works even when
# ~/.local/bin isn't on the desktop session's PATH.
mkdir -p "$(dirname "$desktop")"
sed "s|^Exec=neoviz$|Exec=$bin_dir/neoviz|" "$here/neoviz.desktop" > "$desktop"
chmod 644 "$desktop"
refresh_menu

echo "Installed neoViz to $bin_dir/neoviz and added it to the app menu."
case ":$PATH:" in
    *":$bin_dir:"*) ;;
    *) echo "Note: $bin_dir isn't on your PATH, so 'neoviz' won't run from a terminal by name." ;;
esac
