# neoViz

A retro-style music visualizer for Windows and Linux. It listens to whatever your computer is playing (Spotify, a browser, a game) and draws full-screen visuals in the spirit of the mid-2000s media player visualizers. It doesn't play music itself.

> **Status:** early development. Milestone 1 (window, GPU setup, full-screen) is in place; audio capture is next. See [SCOPE.md](SCOPE.md) for the full plan.

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
neoviz [--fullscreen] [--monitor N]
```

| Key | Action |
|---|---|
| `F` / double-click | Toggle full screen |
| `Esc` | Leave full screen, or quit when windowed |

Set `RUST_LOG=debug` for verbose logging.
