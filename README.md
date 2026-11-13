# neoViz

A retro-style music visualizer for Windows and Linux. It listens to whatever your computer is playing (Spotify, a browser, a game) and draws full-screen visuals in the spirit of the mid-2000s media player visualizers. It doesn't play music itself.

> **Status:** early development. System audio capture works on Linux and Windows and currently drives a debug view (waveform on top, spectrum below). Visualizers come next. See [SCOPE.md](SCOPE.md) for the full plan.

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
neoviz [--fullscreen] [--monitor N] [--device NAME]
```

By default neoViz captures whatever is playing on your default output device and follows it if you switch outputs. Use `--device` to capture something else, such as a microphone:

- Linux: a PulseAudio source name from `pactl list short sources`.
- Windows: the device name as shown in Sound settings.

| Key | Action |
|---|---|
| `F` / double-click | Toggle full screen |
| `Esc` | Leave full screen, or quit when windowed |

Set `RUST_LOG=debug` for verbose logging.
