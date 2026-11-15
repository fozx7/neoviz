#!/usr/bin/env sh
# Installs neoViz for the current user (no root needed):
#   binary  -> ~/.local/bin/neoviz
#   menu    -> ~/.local/share/applications/neoviz.desktop
#   icon    -> ~/.local/share/icons/hicolor/{scalable,512x512}/apps/neoviz.{svg,png}
#   PATH    -> if ~/.local/bin isn't on it, a marked line in your shell's
#              startup file (~/.bashrc, ~/.zshrc, fish conf.d, or ~/.profile)
#
# Usage:
#   ./install.sh [--no-path] [path/to/neoviz]
#                       install (default binary: the one next to this script,
#                       or target/dist/neoviz in the repo); --no-path leaves
#                       your shell startup files alone
#   ./install.sh --uninstall
#                       remove everything installed above
set -eu

here="$(cd "$(dirname "$0")" && pwd)"
bin_dir="$HOME/.local/bin"
data_dir="${XDG_DATA_HOME:-$HOME/.local/share}"
desktop="$data_dir/applications/neoviz.desktop"
icon_dir="$data_dir/icons/hicolor"
icon_svg="$icon_dir/scalable/apps/neoviz.svg"
icon_png="$icon_dir/512x512/apps/neoviz.png"
# Marks the PATH line we add, so uninstall can find and remove it.
path_mark="# Added by neoViz install.sh"
fish_conf="${XDG_CONFIG_HOME:-$HOME/.config}/fish/conf.d/neoviz.fish"

refresh_menu() {
    command -v update-desktop-database >/dev/null 2>&1 \
        && update-desktop-database "$data_dir/applications" >/dev/null 2>&1 || true
    # -t: the per-user hicolor folder has no index.theme of its own. Without
    # it the cache isn't rebuilt, and a stale one can hide the new icon.
    command -v gtk-update-icon-cache >/dev/null 2>&1 \
        && gtk-update-icon-cache -f -t -q "$icon_dir" >/dev/null 2>&1 || true
    command -v kbuildsycoca6 >/dev/null 2>&1 && kbuildsycoca6 >/dev/null 2>&1 || true
    # KDE: running apps (the menu, the taskbar) reload icons on this signal,
    # as they do when the icon theme is changed in System Settings.
    command -v dbus-send >/dev/null 2>&1 \
        && dbus-send --session --type=signal /KIconLoader \
            org.kde.KIconLoader.iconChanged int32:0 >/dev/null 2>&1 || true
}

# The startup file for the user's login shell (fish is handled separately).
shell_rc() {
    case "$(basename "${SHELL:-sh}")" in
        bash) echo "$HOME/.bashrc" ;;
        zsh) echo "${ZDOTDIR:-$HOME}/.zshrc" ;;
        *) echo "$HOME/.profile" ;;
    esac
}

add_to_path() {
    case ":$PATH:" in
        *":$bin_dir:"*) return ;;
    esac
    if [ "$(basename "${SHELL:-sh}")" = fish ]; then
        mkdir -p "$(dirname "$fish_conf")"
        printf '%s\nfish_add_path -g "$HOME/.local/bin"\n' "$path_mark" > "$fish_conf"
        echo "Added ~/.local/bin to your PATH ($fish_conf). Open a new terminal to use 'neoviz'."
        return
    fi
    rc="$(shell_rc)"
    if ! grep -qF "$path_mark" "$rc" 2>/dev/null; then
        printf '\n%s\nexport PATH="$HOME/.local/bin:$PATH"\n' "$path_mark" >> "$rc"
    fi
    echo "Added ~/.local/bin to your PATH in $rc. Open a new terminal (or run: . $rc) to use 'neoviz'."
}

remove_from_path() {
    for rc in "$HOME/.bashrc" "${ZDOTDIR:-$HOME}/.zshrc" "$HOME/.profile"; do
        if [ -f "$rc" ] && grep -qF "$path_mark" "$rc"; then
            # Drop the marker and the export line after it (and the blank
            # line we added before them), keeping everything else as it was.
            awk -v mark="$path_mark" '
                $0 == mark { skip = 1; if (n > 0 && lines[n] == "") n--; next }
                skip { skip = 0; next }
                { lines[++n] = $0 }
                END { for (i = 1; i <= n; i++) print lines[i] }' "$rc" > "$rc.neoviz-tmp"
            cat "$rc.neoviz-tmp" > "$rc" && rm -f "$rc.neoviz-tmp"
        fi
    done
    if [ -f "$fish_conf" ] && grep -qF "$path_mark" "$fish_conf"; then
        rm -f "$fish_conf"
    fi
}

if [ "${1:-}" = "--uninstall" ]; then
    rm -f "$bin_dir/neoviz" "$desktop" "$icon_svg" "$icon_png"
    remove_from_path
    refresh_menu
    echo "neoViz uninstalled. (Settings in ~/.config/neoviz were left in place.)"
    exit 0
fi

update_path=1
if [ "${1:-}" = "--no-path" ]; then
    update_path=0
    shift
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
if [ "$update_path" = 1 ]; then
    add_to_path
else
    case ":$PATH:" in
        *":$bin_dir:"*) ;;
        *) echo "Note: $bin_dir isn't on your PATH, so 'neoviz' won't run from a terminal by name." ;;
    esac
fi
