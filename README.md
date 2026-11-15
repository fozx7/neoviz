# neoViz

A retro-style music visualizer for Windows and Linux. It listens to whatever your computer is playing (Spotify, a browser, a game) and draws full-screen visuals in the spirit of the mid-2000s media player visualizers. It doesn't play music itself.

It comes with seven built-in visualizers (plus variants) and can also play **MilkDrop** presets, the thousands of community-made visuals from Winamp's classic visualizer.

## Install

Download the latest package from the repo's **Releases** page. There are x86-64 and ARM64 (`arm64`) builds for each OS.

**Windows:** unzip anywhere and run `neoviz.exe`.

**Linux:** unpack and run the installer. It puts `neoviz` in `~/.local/bin`, adds neoViz to your app menu, and (if `~/.local/bin` isn't already on your `PATH`) adds it to your shell's startup file so `neoviz` runs from a terminal. No root needed:

```sh
tar xzf neoviz-*-linux-x86_64.tar.gz
cd neoviz-*-linux-x86_64 && ./install.sh
```

Open a new terminal afterwards to run `neoviz` by name. `./install.sh --no-path` skips the `PATH` change; `./install.sh --uninstall` removes everything again, including that line (your settings are kept). Needs PulseAudio or PipeWire, and OpenGL/EGL, which any desktop has.

Start playing music and neoViz reacts straight away; there's nothing to set up. Press `Tab` for settings.

## MilkDrop presets

MilkDrop support is built in, but the presets themselves aren't included. A good start is the "cream of the crop" pack (about 9,800 presets, 22 MB).

### Windows

1. Download the pack as a zip: <https://github.com/projectM-visualizer/presets-cream-of-the-crop/archive/refs/heads/master.zip>
2. Unzip it somewhere permanent, for example `C:\Users\<you>\milkdrop-presets`. (It's fine if that leaves a `presets-cream-of-the-crop-master` folder inside; subfolders are searched.)
3. In neoViz, press `Tab`, paste the folder path into **MilkDrop → Preset folder**, and press Enter (or click **Rescan**).

### Linux

```sh
git clone --depth 1 https://github.com/projectM-visualizer/presets-cream-of-the-crop ~/milkdrop-presets
```

Then press `Tab` in neoViz and enter `~/milkdrop-presets` under **MilkDrop → Preset folder** (or start it once with `neoviz --presets ~/milkdrop-presets`).

### Using MilkDrop

The folder is remembered, and MilkDrop appears as one more visualizer (`←` / `→` to reach it). By default it shuffles through the presets, changing every 30 s; in the overlay you can turn shuffle or the timer off, or change the interval. `[` / `]` step back and forward through presets, `Space` holds the current one, and each new preset's name pops up briefly. The next preset is loaded in the background while the current one plays, so changes crossfade smoothly. Presets that can't be loaded are skipped.

Any folder of `.milk` files works, so you can add other packs or your own.

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

## Building from source

You need a recent stable Rust toolchain ([rustup.rs](https://rustup.rs)).

```sh
cargo run --release
```

MilkDrop comes from [libprojectM](https://github.com/projectM-visualizer/projectm), which is built from source as part of the normal build, so the first build takes a while. To build without it, use `--no-default-features`.

**Linux:** install the PulseAudio client libraries (they also work on PipeWire via `pipewire-pulse`), plus `cmake`, a C++ compiler and the OpenGL/EGL headers for MilkDrop:

- Arch: `sudo pacman -S libpulse base-devel cmake mesa`
- Debian/Ubuntu: `sudo apt install libpulse-dev cmake g++ libegl-dev libgl-dev`

To install your own build: `cargo build --profile dist`, then `packaging/linux/install.sh`.

**Windows:** the Rust toolchain (MSVC) is enough with `--no-default-features`. For MilkDrop you also need Visual Studio 2022 or later (C++ workload), CMake, LLVM (for `libclang`), and [vcpkg](https://vcpkg.io) with `VCPKG_INSTALLATION_ROOT` pointing at it; vcpkg fetches GLEW by itself during the build. The repo carries patched projectM build crates for this (see [vendor/README.md](vendor/README.md)).

## App icon

All icon files are generated from one source image: run `packaging/make-icons.sh icon.svg` (or a square PNG of 512 px or more; needs ImageMagick, plus `rsvg-convert` for SVG). It writes the window icon built into the binary, the Windows `.exe` icon and the Linux app-menu icon. On Wayland the window's icon comes from the installed app-menu entry, so it shows once neoViz is installed with `install.sh`.

## Releasing

Bump `version` in `Cargo.toml`, commit, then tag and push:

```sh
git tag v1.0.0 && git push origin v1.0.0
```

The Release workflow builds optimised Linux and Windows packages (x86-64 and ARM64) and publishes them as a GitHub release.

## Licence

neoViz is MIT-licensed: anyone can take the source and make it their own.

MilkDrop support links [projectM](https://github.com/projectM-visualizer/projectm), which is LGPL-2.1, statically. The LGPL lets you swap in your own projectM: neoViz's full source (including the patched build crates in `vendor/`) is available, and the release packages include the licence text and [third-party notices](packaging/THIRD-PARTY-NOTICES.md).
