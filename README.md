# neoViz

A retro-style music visualizer for Windows and Linux. It listens to whatever your computer is playing (Spotify, a browser, a game) and draws full-screen visuals in the spirit of the mid-2000s media player visualizers. It doesn't play music itself.

> **Status:** usable. Audio capture, all seven built-in visualizers (plus variants), crossfades, post-FX, a settings overlay, saved settings and optional MilkDrop presets (Linux) are in. Packaging is next. See [SCOPE.md](SCOPE.md) for the full plan.

## Build

You need a recent stable Rust toolchain ([rustup.rs](https://rustup.rs)).

```sh
cargo run --release
```

### Linux

Install the PulseAudio client libraries (they also work on PipeWire via `pipewire-pulse`):

- Arch: `sudo pacman -S libpulse`
- Debian/Ubuntu: `sudo apt install libpulse-dev`

### Windows

Nothing extra beyond the Rust toolchain (MSVC).

### MilkDrop presets (Linux)

On Linux, neoViz plays MilkDrop presets through [libprojectM](https://github.com/projectM-visualizer/projectm), built from source as part of the normal build (the first build takes ~30 s longer). You'll need `cmake`, a C++ compiler and the OpenGL/EGL headers (on Arch these come with `base-devel`, `cmake` and `mesa`; on Debian/Ubuntu: `cmake g++ libegl-dev libgl-dev`).

```sh
cargo run --release -- --presets ~/milkdrop-presets
```

Presets aren't included. A good start is the "cream of the crop" pack:

```sh
git clone --depth 1 https://github.com/projectM-visualizer/presets-cream-of-the-crop ~/milkdrop-presets
```

The folder can also be set in the settings overlay. MilkDrop then appears as one more visualizer. By default it shuffles through the presets, changing every 30 s; in the overlay you can turn shuffle or the timer off, or change the interval. `[` / `]` step back and forward through presets, and each new preset's name pops up briefly. projectM runs on its own thread, so slow-loading presets don't freeze the app.

projectM is LGPL-3.0 and is linked statically into MilkDrop builds. That's fine for personal use; if you distribute such a build, the LGPL requires that recipients can relink it against their own projectM. To build without it, use `--no-default-features`.

## Usage

```sh
neoviz [--fullscreen] [--span] [--monitor N] [--device NAME] [--visualizer NAME] [--presets DIR] [--list-devices]
```

By default neoViz captures whatever is playing on your default output device and follows it if you switch outputs. Use `--device` to capture something else, such as a microphone:

- Linux: a PulseAudio source name (run `neoviz --list-devices`).
- Windows: the device name as shown in Sound settings (or `--list-devices`).

You can also pick the device in the settings overlay (`Tab`).

| Key | Action |
|---|---|
| `F` / double-click | Toggle full screen |
| `F` `F` (double-tap) | Span every monitor (one window per monitor, one picture across them); `F` or `Esc` to leave |
| `←` / `→` | Previous / next visualizer |
| `Esc` | Leave full screen, or quit when windowed |
| `↑` / `↓` | Sensitivity up / down (3 dB steps) |
| `R` | Toggle random mode (auto-switch every N seconds) |
| `Space` | Lock the current visualizer (pause random mode) |
| `Tab` | Settings overlay |
| `B` | Toggle bloom |
| `C` | Toggle CRT scanlines |
| `[` / `]` | Previous / next MilkDrop preset |
| `D` | Toggle the analysis debug view |

Visualizers: Bars & Peaks, Oscilloscope, Ambient Swirl, Battery Rings, Alchemy Particles, Classic Lightshow, Starfield Warp, then variants: LED Bars, Ring Scope, Mirror Bars, Stereo X/Y.

Settings are saved automatically to `~/.config/neoviz/settings.toml` (Linux) or `%APPDATA%\neoviz\config\settings.toml` (Windows). The audio device is the exception: every launch starts on the current default output, and a device picked in the overlay or with `--device` lasts for that run only.

Set `RUST_LOG=debug` for verbose logging.
