# neoViz: Scope Document

## 1. Summary

**neoViz** is a small standalone desktop app for **Windows and Linux** (macOS is a planned later addition, see §10). It listens to whatever audio the computer is playing (Spotify, a browser, a game, anything) and draws full-screen visualizations that react to it, in the style of the mid-2000s Windows Media Player and iTunes visualizers. It doesn't play music. It only listens to the system output and draws.

This is a personal project, so keep it simple. A single binary per platform is better than a polished installer.

## 2. Goals

- Capture **system audio output** (loopback). Don't use the microphone.
- Run smooth visuals at **60 fps**, at any window size and in full screen, with low latency (under ~50 ms from sound to picture).
- Ship a handful of built-in visualizers with a retro feel, and make it easy to add more.
- Support MilkDrop presets (the Winamp classic) through **projectM**. This brings in thousands of existing community presets.
- Offer simple controls: next/previous visualizer, random mode, full screen, and an on-screen overlay.
- Build from one codebase for Windows and Linux, structured so a macOS backend can be added later without reworking the app.

## 3. Non-goals (v1)

- No music playback, library, or playlists.
- No "now playing" track info (possible stretch goal, see §10).
- No plugin compatibility with the real WMP or iTunes visualizer formats.
- No macOS support in v1 (deferred, see §10).
- No app store distribution, code signing, or auto-update.
- No pixel-for-pixel copies of the Microsoft or Apple visualizers. Build originals *inspired by* them, with no trademarked names in the UI.

## 4. Recommended tech stack

| Concern | Choice | Notes |
|---|---|---|
| Language | **Rust** | One codebase that cross-compiles, single static-ish binary. (C++ is an acceptable alternative if projectM integration proves painful in Rust.) |
| Window + input | `winit` | Cross-platform windowing, full-screen toggle, keyboard. |
| Rendering | `wgpu` (WGSL shaders) | Vulkan/DX12/Metal under one API. |
| Simple UI overlay | `egui` (via `egui-wgpu`) | Settings panel and the on-screen name of the current visualizer. |
| FFT | `rustfft` or `realfft` | |
| Audio capture | Per-platform backend behind a trait (see §5) | |
| MilkDrop presets | `libprojectM` v4 (C API) via FFI or an existing Rust binding | Feature-flagged so the app builds without it. |
| Config | `serde` + TOML in the OS config dir (`directories` crate) | |

## 5. Audio capture: the hard part

Define one trait so the rest of the app doesn't care about the OS:

```rust
trait AudioSource {
    fn start(&mut self, sink: RingBufferProducer<f32>) -> Result<()>;
    fn stop(&mut self);
    fn sample_rate(&self) -> u32;
    fn list_devices() -> Vec<DeviceInfo>;
}
```

| OS | Approach | Notes |
|---|---|---|
| **Windows** | WASAPI **loopback** on the default render device | Well supported. `cpal` can open an output device as an input stream on WASAPI. Or call the `windows` crate directly. Follow default-device changes (e.g. when headphones are plugged in). |
| **Linux** | Capture the **monitor source** of the default sink via PulseAudio (works on PipeWire through `pipewire-pulse`) | Use `libpulse-binding` or `pipewire` crate. Pick `@DEFAULT_MONITOR@`. |

Also:
- macOS is out of scope for v1 (see §10). Keep all OS-specific code behind the `AudioSource` trait and `#[cfg(target_os)]` so a macOS backend can drop in later.
- Add a manual **input device picker** as a fallback on every OS. This also lets the user visualize a mic or line-in.
- Downmix to mono for analysis, but keep stereo available (some visualizers use L/R separately).
- Keep the audio thread lock-free: audio callback → ring buffer → analysis on the render thread (or its own thread).

## 6. Audio analysis

Compute these each frame and hand them to visualizers as a read-only `AudioFrame`:

- **Waveform**: the latest ~1024–2048 samples (per channel).
- **Spectrum**: FFT size 2048 with a Hann window, magnitudes in dB, plus a **log-spaced band set** (e.g. 32 / 64 / 128 bands) with smoothing (fast attack, slow decay) and peak-hold values.
- **Levels**: RMS, peak, and bass/mid/treble energy.
- **Beat detection**: a simple energy-based onset detector on the bass band, giving a `beat: bool` plus `beat_intensity`. Tempo estimation is optional.
- **Silence detection**: after N seconds of silence, fade to an idle animation.

Sensitivity/gain and smoothing are user settings.

## 7. Visualizers

### Architecture

```rust
trait Visualizer {
    fn name(&self) -> &str;
    fn init(&mut self, gpu: &GpuContext);
    fn resize(&mut self, w: u32, h: u32);
    fn update(&mut self, audio: &AudioFrame, dt: f32);
    fn render(&mut self, target: &RenderTarget);
}
```

- Registry of visualizers, cycled with the arrow keys or at random.
- **Crossfade** of about 1 s when switching. Render both into offscreen textures and blend.
- Shared helpers: a feedback buffer (draw the previous frame, zoomed/rotated/faded, under the current one, which is the key to the "old school" look), colour palette cycling, and post-processing (bloom/glow, optional CRT scanlines).

### Built-in set for v1 (originals, styled after the classics)

1. **Bars & Peaks**: a classic spectrum analyser with falling peak caps. Variants: solid, segmented LED, mirrored.
2. **Oscilloscope Wave**: a glowing waveform line with feedback trails. Variants: line, circular, stereo X/Y (Lissajous).
3. **Ambient Swirl**: soft flowing colour fields driven by bass energy, with a feedback-buffer zoom/rotate.
4. **Battery Rings**: concentric pulsing geometric rings and spokes whose radius and colour react to bands and beats.
5. **Alchemy Particles**: a particle field that bursts on beats, with trails and additive blending.
6. **Classic Lightshow**: iTunes-classic-style flowing ribbons and waveform blooms over a fading feedback trail.
7. **Starfield Warp**: stars stream toward the viewer, with speed tied to energy and colour flashes on beats.

### MilkDrop via projectM (feature flag `milkdrop`)

- Wrap projectM as one more `Visualizer`. Feed it PCM and render to the wgpu surface (projectM uses OpenGL, so either share a GL context or render it to a texture and copy it across. Pick whichever is simplest. Running a separate GL window in "MilkDrop mode" is acceptable for v1).
- Ship with a small curated preset pack (check the licence; the projectM "cream of the crop" pack is a good start), and let the user point at their own preset folder.
- If this turns into a time sink, defer it to v1.1. The built-in visualizers are the core.

## 8. App behaviour and controls

| Key | Action |
|---|---|
| `F` / double-click | Toggle full screen |
| `←` / `→` | Previous / next visualizer |
| `R` | Toggle random mode (auto-switch every N seconds, and optionally on a strong beat) |
| `Space` | Lock the current visualizer (pause auto-switch) |
| `↑` / `↓` | Sensitivity up / down |
| `Tab` | Show/hide the settings overlay (device, sensitivity, smoothing, auto-switch interval, which visualizers are in rotation, post-FX toggles, FPS counter) |
| `Esc` | Leave full screen, or quit when windowed |

Also:
- The current visualizer's name fades in for 2 s on switch.
- Settings persist between runs.
- Optional "always on top" and borderless window mode, so it can sit on a second monitor.
- CLI flags: `--fullscreen`, `--monitor N`, `--visualizer <name>`, `--device <name>`.

## 9. Performance and quality targets

- Steady 60 fps at 1080p on integrated graphics (Intel Iris Xe / AMD Vega class), and 4K on a discrete GPU.
- Under 5% CPU on a modern machine (no CPU-side per-pixel work).
- Survives audio device changes, sleep/wake, and window resizes without crashing or a black screen.
- Shows a clear on-screen message (not a crash) when capture fails or permission is denied.

## 10. Stretch goals (post-v1)

- **macOS support.** Primary: a **Core Audio process tap** (macOS 14.2+) tapping all system output. Fallback: **ScreenCaptureKit** audio-only capture (macOS 13+). Last resort: the user installs a virtual loopback device (e.g. BlackHole) and picks it as input. Expect a permission prompt; handle a denial with a clear on-screen message. Needs a small Objective-C/Swift shim or `objc2` bindings, an unsigned `.app` bundle, and a third CI target. Because rendering uses wgpu (Metal on macOS), the rest of the app should run unchanged.

- "Now playing" overlay via OS media APIs (Windows SMTC, Linux MPRIS, macOS MediaRemote/Now Playing).
- Hot-reload of WGSL shaders from a user folder, so new visualizers can be written without recompiling.
- A screensaver mode (Windows `.scr`).
- Recording or exporting a clip to video.
- Album-art-tinted colour palettes.

## 11. Suggested project structure

```
neoviz/
├── Cargo.toml
├── src/
│   ├── main.rs            # arg parsing, event loop
│   ├── app.rs             # state, input handling, visualizer switching/crossfade
│   ├── config.rs
│   ├── audio/
│   │   ├── mod.rs         # AudioSource trait, ring buffer
│   │   ├── windows.rs     # WASAPI loopback
│   │   ├── linux.rs       # Pulse/PipeWire monitor
│   │   ├── (macos.rs)     # later: Core Audio tap / ScreenCaptureKit
│   │   └── analysis.rs    # FFT, bands, beat detection
│   ├── render/
│   │   ├── gpu.rs         # wgpu setup
│   │   ├── feedback.rs    # feedback buffer helper
│   │   └── postfx.rs      # bloom, scanlines
│   ├── viz/
│   │   ├── mod.rs         # Visualizer trait, registry
│   │   ├── bars.rs
│   │   ├── scope.rs
│   │   ├── ambient.rs
│   │   ├── rings.rs
│   │   ├── particles.rs
│   │   ├── lightshow.rs
│   │   ├── starfield.rs
│   │   └── milkdrop.rs    # #[cfg(feature = "milkdrop")]
│   └── ui/overlay.rs      # egui
└── shaders/*.wgsl
```

## 12. Milestones

1. **Skeleton**: window, wgpu clear colour, full-screen toggle, CI building on Windows and Linux (GitHub Actions matrix).
2. **Audio in**: Windows and Linux loopback working, with a debug view of the raw waveform and spectrum.
3. **Analysis**: bands, smoothing, beat detection, plus a debug overlay.
4. **First two visualizers**: Bars & Peaks and Oscilloscope Wave, plus switching and crossfade.
5. **Remaining built-in visualizers**, the feedback buffer, and post-FX.
6. **Settings overlay and persistence.**
7. **projectM / MilkDrop** (feature-flagged).
8. **Packaging**: Windows `.exe` (zip), Linux AppImage or plain binary.
9. *(Later)* **macOS**: see §10.

## 13. Acceptance criteria for v1

- [ ] Play music in any app on Windows and Linux, launch the visualizer, and it reacts within a second with no configuration.
- [ ] At least 5 built-in visualizers, switchable with the keyboard, with crossfades.
- [ ] Full-screen and windowed modes both work, including on a second monitor.
- [ ] Settings persist across restarts.
- [ ] Unplugging headphones or changing the default output device doesn't kill the visuals.
- [ ] A README covers build steps for Windows and Linux.

## 14. Notes for the implementer

- **Do the audio capture early.** It's the riskiest part.
- Don't build anything macOS-specific yet, but avoid choices that would block it later (e.g. no Windows-only or Linux-only rendering or UI code outside the audio backends).
- Prefer GPU shaders for all visual work, and keep visualizer code small and self-contained so adding a new one is a single file plus a shader.
- Keep dependencies modest. Ask before pulling in a large framework (game engine, Electron, etc.).
