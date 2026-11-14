# neoViz

A retro-style music visualizer for Windows and Linux. It listens to whatever your computer is playing (Spotify, a browser, a game) and draws full-screen visuals in the spirit of the mid-2000s media player visualizers. It doesn't play music itself.

> **Status:** usable. Audio capture, all seven built-in visualizers (plus variants), crossfades, post-FX, a settings overlay, saved settings and optional MilkDrop presets are in. Packaging is next. See [SCOPE.md](SCOPE.md) for the full plan.

## Install

Download the latest package from the repo's **Releases** page. There are x86-64 and ARM64 (`arm64`) builds for each OS.

**Linux:** unpack and run the installer. It puts `neoviz` in `~/.local/bin` and adds neoViz to your app menu (no root needed):

```sh
tar xzf neoviz-*-linux-x86_64.tar.gz
cd neoviz-*-linux-x86_64 && ./install.sh
```

`./install.sh --uninstall` removes it again (your settings are kept). Needs PulseAudio or PipeWire, and OpenGL/EGL, which any desktop has.

**Windows:** unzip anywhere and run `neoviz.exe`.

To install your own build instead: `cargo build --profile dist`, then `packaging/linux/install.sh` from the repo.


You need a recent stable Rust toolchain ([rustup.rs](https://rustup.rs)).

```sh
cargo run --release
```

### Linux

Install the PulseAudio client libraries (they also work on PipeWire via `pipewire-pulse`):

- Arch: `sudo pacman -S libpulse`
- Debian/Ubuntu: `sudo apt install libpulse-dev`

### Windows

The Rust toolchain (MSVC) is enough without MilkDrop (`--no-default-features`). For MilkDrop, see below.

### MilkDrop presets

neoViz plays MilkDrop presets through [libprojectM](https://github.com/projectM-visualizer/projectm), which is built from source as part of the normal build. The first build takes longer.

- **Linux:** you need `cmake`, a C++ compiler and the OpenGL/EGL headers. On Arch these come with `base-devel`, `cmake` and `mesa`; on Debian/Ubuntu install `cmake g++ libegl-dev libgl-dev`.
- **Windows:** you need Visual Studio 2022 or later (C++ workload), CMake, LLVM (for `libclang`), and [vcpkg](https://vcpkg.io) with `VCPKG_INSTALLATION_ROOT` pointing at it. vcpkg fetches GLEW by itself during the build. The repo carries a patched `projectm-sys` for this (see [vendor/README.md](vendor/README.md)).

```sh
cargo run --release -- --presets ~/milkdrop-presets
```

Presets aren't included. A good start is the "cream of the crop" pack:

```sh
git clone --depth 1 https://github.com/projectM-visualizer/presets-cream-of-the-crop ~/milkdrop-presets
```

The folder can also be set in the settings overlay. MilkDrop then appears as one more visualizer. By default it shuffles through the presets, changing every 30 s; in the overlay you can turn shuffle or the timer off, or change the interval. `[` / `]` step back and forward through presets, and each new preset's name pops up briefly. The next preset is loaded in the background while the current one plays, so changes crossfade smoothly (2 s on the timer, half a second for `[` / `]`) instead of pausing while a preset compiles. Presets projectM can't load are skipped.

projectM is LGPL-3.0 and is linked statically into MilkDrop builds. That's fine for personal use; if you distribute such a build, the LGPL requires that recipients can relink it against their own projectM. To build without it, use `--no-default-features`.

## Usage

```sh
neoviz [--fullscreen] [--span] [--monitor N] [--device NAME] [--visualizer NAME] [--presets DIR] [--list-devices]
```

By default neoViz captures whatever is playing on your default output device and follows it if you switch outputs. Use `--device` to capture something else, such as a microphone:

- Linux: a PulseAudio source name (run `neoviz --list-devices`).
- Windows: the device name as shown in Sound settings (`--list-devices` prints nothing from release builds, which have no console; use the overlay's device list).

You can also pick the device in the settings overlay (`Tab`).

| Key | Action |
|---|---|
| `F` / double-click | Toggle full screen |
| `F` `F` (double-tap) | Span every monitor (one window per monitor, one picture across them); `F` or `Esc` to leave |
| `←` / `→` | Previous / next visualizer |
| `Esc` | Leave full screen, or quit when windowed |
| `↑` / `↓` | Sensitivity up / down (6 dB steps; MilkDrop presets set their own levels) |
| `R` | Toggle random mode (auto-switch every N seconds) |
| `Space` | Lock the current visualizer (pause random mode and MilkDrop preset changes) |
| `Tab` | Settings overlay |
| `B` | Toggle bloom |
| `C` | Toggle CRT scanlines |
| `[` / `]` | Previous / next MilkDrop preset |
| `D` | Toggle the analysis debug view |

Visualizers: Bars & Peaks, Oscilloscope, Ambient Swirl, Battery Rings, Alchemy Particles, Classic Lightshow, Starfield Warp, then variants: LED Bars, Ring Scope, Mirror Bars, Stereo X/Y.

**Keep screen on** (overlay → Display): by default neoViz stops the screen blanking or locking while music plays, like a video player, and lets it sleep after a minute of silence. Set it to *Always* or *Off* if you prefer.

**FPS counter** (overlay → Display): shows the frame rate. Tick *Detailed* for performance figures over the last 10 s: 1% and 5% lows (the frame rate across the slowest 1% / 5% of frames), frame times (average, 99th percentile, worst), how much of each frame neoViz spends working versus waiting for the display, how many frames were late (missed a refresh, so the previous picture showed twice), and a graph of recent frame times with late frames in orange. If frames are late while *work* is small, something outside neoViz is holding it up.

**Fade out when the music stops** (overlay → Display): after a couple of seconds of silence the picture fades to black, and it fades back in as soon as sound returns. Untick it to keep the visuals running in silence.

Too fast or too flashy? In the overlay's **Motion** section, *Speed* slows the visualizers' own movement and colour cycling (0.25×–2×), and *Beat flashes* tones down the pulse on each beat. MilkDrop presets keep their own pace.

**Span mode on KDE Plasma (Wayland):** by default KWin gives a newly opened window focus, and when it closes, passes focus only to windows on the *same* monitor. So after leaving span mode, neoViz may need a click before keys work again. To avoid that, turn off *System Settings → Window Management → Window Behavior → Focus → Separate screen focus*.

Settings are saved automatically to `~/.config/neoviz/settings.toml` (Linux) or `%APPDATA%\neoviz\config\settings.toml` (Windows). The audio device is the exception: every launch starts on the current default output, and a device picked in the overlay or with `--device` lasts for that run only.

Each run writes a log to `neoviz.log` next to the settings file, which is the place to look if something doesn't start (on Windows there's no console). If MilkDrop can't start, the reason also shows in the overlay's MilkDrop section. Set `RUST_LOG=debug` for verbose logging.

## App icon

All icon files are generated from one source image: run `packaging/make-icons.sh icon.svg` (or a square PNG of 512 px or more; needs ImageMagick, plus `rsvg-convert` for SVG). It writes the window icon built into the binary, the Windows `.exe` icon and the Linux app-menu icon. On Wayland the window's icon comes from the installed app-menu entry, so it shows once neoViz is installed with `install.sh`.

## Releasing

Bump `version` in `Cargo.toml`, commit, then tag and push:

```sh
git tag v1.0.0 && git push origin v1.0.0
```

The Release workflow builds optimised Linux and Windows packages (x86-64 and ARM64) and publishes them as a GitHub release.
