# neoViz

A retro-style music visualizer for Windows and Linux. It listens to whatever your computer is playing (Spotify, a browser, a game) and draws full-screen visuals in the spirit of the mid-2000s media player visualizers. It doesn't play music itself.

> **Status:** usable. Audio capture, all seven built-in visualizers (plus variants), crossfades, post-FX, a settings overlay and saved settings are in. MilkDrop presets and packaging are next. See [SCOPE.md](SCOPE.md) for the full plan.

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

## Usage

```sh
neoviz [--fullscreen] [--monitor N] [--device NAME] [--visualizer NAME] [--list-devices]
```

By default neoViz captures whatever is playing on your default output device and follows it if you switch outputs. Use `--device` to capture something else, such as a microphone:

- Linux: a PulseAudio source name (run `neoviz --list-devices`).
- Windows: the device name as shown in Sound settings (or `--list-devices`).

You can also pick the device in the settings overlay (`Tab`).

| Key | Action |
|---|---|
| `F` / double-click | Toggle full screen |
| `←` / `→` | Previous / next visualizer |
| `Esc` | Leave full screen, or quit when windowed |
| `↑` / `↓` | Sensitivity up / down (3 dB steps) |
| `R` | Toggle random mode (auto-switch every N seconds) |
| `Space` | Lock the current visualizer (pause random mode) |
| `Tab` | Settings overlay |
| `B` | Toggle bloom |
| `C` | Toggle CRT scanlines |
| `D` | Toggle the analysis debug view |

Visualizers: Bars & Peaks, Oscilloscope, Ambient Swirl, Battery Rings, Alchemy Particles, Classic Lightshow, Starfield Warp, then variants: LED Bars, Ring Scope, Mirror Bars, Stereo X/Y.

Settings are saved automatically to `~/.config/neoviz/settings.toml` (Linux) or `%APPDATA%\neoviz\config\settings.toml` (Windows). The audio device is the exception: every launch starts on the current default output, and a device picked in the overlay or with `--device` lasts for that run only.

Set `RUST_LOG=debug` for verbose logging.
