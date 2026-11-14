# neoViz: Scope Document

> **Status (2026-11-14):** **v0.9.0** is released. Every v1 milestone is now done, including MilkDrop on Windows, which runs on real hardware. What's left before 1.0 is in §15: the app icon, a final Windows check, and the release itself. Where the build differs from the original plan, the relevant section says so under **As built**.

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

## 4. Tech stack

| Concern | Choice | As built |
|---|---|---|
| Language | **Rust** | Rust 2024 edition. |
| Window + input | `winit` | `winit` 0.30. |
| Rendering | `wgpu` (WGSL shaders) | `wgpu` 30. |
| UI overlay | `egui` (via `egui-wgpu`) | `egui` 0.37. |
| FFT | `rustfft` or `realfft` | `realfft`. |
| Ring buffer | (unspecified) | `rtrb`, lock-free SPSC. |
| Audio capture | Per-platform backend (see §5) | Windows: `cpal` (WASAPI). Linux: `libpulse-simple-binding`. |
| MilkDrop presets | `libprojectM` v4 | `projectm` crate, built from source and linked statically; hidden EGL context via `khronos-egl` + `glow`. |
| Config | `serde` + TOML via `directories` | As planned. |
| CLI | (unspecified) | `clap`. |
| Screen keep-awake | (not planned) | `keepawake`. |

## 5. Audio capture: the hard part

The original plan was an `AudioSource` trait. **As built:** each OS backend is a `run(Arc<CaptureContext>)` function selected with `#[cfg]`. It runs on its own thread, pushes interleaved stereo `f32` into a lock-free ring buffer, and reconnects on any error (device unplugged, default device changed, sound server restarted) after a 1 s delay. This is simpler than a trait and gives the same seam for a macOS backend.

| OS | Approach | As built |
|---|---|---|
| **Windows** | WASAPI **loopback** on the default render device | Via `cpal`. `cpal` doesn't move a stream when the default device changes, so the backend detects the change itself and reopens. |
| **Linux** | Monitor source of the default sink via PulseAudio (works on PipeWire through `pipewire-pulse`) | `@DEFAULT_MONITOR@`. |

Also:
- Manual **input device picker**: in the overlay and via `--device`. A picked device lasts for that run only; every launch starts on the current default output.
- Stereo is kept end to end. Analysis downmixes to mono, and the Stereo X/Y scope uses L/R.
- Capture failures are shown on screen (and in the overlay) until capture recovers.

## 6. Audio analysis

Computed each frame and handed to visualizers as a read-only `AudioFrame`:

- **Waveform**: the latest samples per channel.
- **Spectrum**: FFT size 2048 with a Hann window, magnitudes in dB, plus log-spaced bands with fast-attack/slow-decay smoothing and peak hold.
- **Levels**: RMS, peak, and bass/mid/treble energy.
- **Beat detection**: an energy-based onset detector on the bass band, giving `beat` plus `beat_intensity`. No tempo estimation.
- **Silence detection**: `AudioFrame::silent` turns on after a few seconds below ~-60 dBFS.

Sensitivity (6 dB steps, about a tenth of the bar height each) and smoothing are user settings. MilkDrop presets rate loudness against their own running average, so sensitivity barely affects them.

**As built:** instead of a separate idle animation, the picture **fades to black** over 3 s once `silent` turns on, and **fades back in** over 0.75 s when sound returns (and at launch). It can be turned off in the overlay (*Fade out when the music stops*). `silent` also drives keep-awake (§8).

## 7. Visualizers

### Architecture

```rust
trait Visualizer {
    fn name(&self) -> &str;
    fn resize(&mut self, device: &wgpu::Device, size: (u32, u32)) {}
    fn update(&mut self, audio: &AudioFrame, dt: f32);
    fn configure(&mut self, settings: &Settings, rescan: bool) {}   // react to settings changes
    fn step_preset(&mut self, direction: isize) -> Option<String> { None } // MilkDrop `[` / `]`
    fn take_message(&mut self) -> Option<String> { None }           // on-screen notices
    fn status(&self) -> Option<String> { None }
    fn render(/* encoder, target, ... */);
}
```

- A registry (`viz::create_all`) and a `Switcher` cycle visualizers with the arrow keys or at random, skipping any the user has taken out of rotation.
- About 1 s **crossfade** on every switch. Both visualizers render offscreen and are blended.
- Shared helpers: a feedback buffer, colour palette cycling, a `Layered` base for the shader-driven visualizers, and post-FX (bloom, CRT scanlines) in the compositor.

### Built-in set (all done)

1. **Bars & Peaks**, plus variants **LED Bars** and **Mirror Bars**.
2. **Oscilloscope**, plus variants **Ring Scope** and **Stereo X/Y** (Lissajous).
3. **Ambient Swirl**.
4. **Battery Rings**.
5. **Alchemy Particles**.
6. **Classic Lightshow**.
7. **Starfield Warp**.

The variants come after the seven main visualizers in the rotation order, giving 11 entries in total (12 with MilkDrop).

### MilkDrop via projectM (feature `milkdrop`)

MilkDrop is required on **both Windows and Linux** for v1.

**As built (Linux and Windows):**

- The feature is on by default on both OSes. projectM renders on its own thread into a hidden OpenGL 3.3 context, and frames are uploaded as a wgpu texture. That makes MilkDrop an ordinary visualizer: crossfades, bloom and the overlay all work on top of it.
- **The hidden context is per OS** (`render/headless_gl/`): an EGL pbuffer on Linux, a WGL pbuffer on Windows (bootstrapped through a hidden window; projectM always draws to framebuffer 0, so a framebuffer object won't do). On Windows the worker also initialises GLEW, which projectM uses but leaves to the host.
- **Patched crates** (`vendor/`, see its README): `projectm-sys` for the Windows build (C runtime, generator, library names, GLEW, ARM64 triplet) and `projectm` for ARM64 Linux (`c_char`).
- **Preloading:** two projectM instances take turns. The one on screen keeps rendering while the other loads the next preset, so changes **crossfade** (2 s on the timer, 0.5 s for `[` / `]`) and never pause while shaders compile. The second instance costs about 55 MB of RAM and 27 MB of GPU memory. Presets that fail to load are skipped and remembered for the session.
- If MilkDrop can't start, the overlay's MilkDrop section says why, and the log file (`neoviz.log` beside the settings) has the details.
- **Sensitivity barely affects MilkDrop:** projectM rates loudness against its own running average, as MilkDrop always did. The overlay says so.
- **No preset pack is bundled.** The user points neoViz at a folder (`--presets` or the overlay; `~` is expanded). The README recommends the projectM "cream of the crop" pack.
- Shuffle (on by default), auto-change every 30 s (adjustable), `[` / `]` to step, and each new preset's name pops up.
- projectM is LGPL-3.0 and linked statically. That's fine for personal use, but a distributed build must let recipients relink it. `--no-default-features` builds without it.

## 8. App behaviour and controls

| Key | Action |
|---|---|
| `F` / double-click | Toggle full screen |
| `F` `F` (double-tap) | **Span mode**: one window per monitor, one picture across them. `F` or `Esc` to leave |
| `←` / `→` | Previous / next visualizer |
| `R` | Toggle random mode (auto-switch every N seconds, and optionally on a strong beat) |
| `Space` | Lock the current visualizer (pause auto-switch, and MilkDrop's preset timer) |
| `↑` / `↓` | Sensitivity up / down |
| `Tab` | Settings overlay |
| `B` / `C` | Toggle bloom / CRT scanlines |
| `[` / `]` | Previous / next MilkDrop preset |
| `D` | Analysis debug view |
| `Esc` | Leave full screen, or quit when windowed |

The settings overlay covers the device, sensitivity, smoothing, random mode, the auto-switch interval, switching on beats, which visualizers are in rotation, bloom, scanlines, the FPS counter, always-on-top, borderless, keep-awake, Motion, and the MilkDrop options.

Also:
- The current visualizer's name fades in for 2 s on switch.
- Settings persist between runs (`settings.toml` in the OS config dir), except the audio device (see §5).
- Optional "always on top" and borderless window mode.
- CLI flags: `--fullscreen`, `--span`, `--monitor N`, `--visualizer <name>`, `--device <name>`, `--presets <dir>`, `--list-devices`.

**Added beyond the original scope:**
- **Span mode** across all monitors (above).
- **Keep screen on** (*While music plays* / *Always* / *Off*). By default it stops the screen blanking or locking while there's sound, and lets the screen sleep after a minute of silence, like a video player.
- **Motion** settings. *Speed* (0.25×–2×) slows the visualizers' own movement and colour cycling, and *Beat flashes* tones down the pulse on each beat. MilkDrop presets, and their timer, keep their own pace.
- **Detailed FPS counter**: besides the plain frame rate, 1% / 5% lows, frame times, work vs waiting-on-display per frame, late frames, a frame-time graph, and what's on screen (including the MilkDrop preset).
- **Log file** (`neoviz.log` beside the settings), since Windows release builds have no console.
- **App icon** on every window, the taskbar and the Windows `.exe`; generated from one source by `packaging/make-icons.sh`.
- **ARM64 builds** for Linux and Windows, alongside x86-64.

## 9. Performance and quality targets

- Steady 60 fps at 1080p on integrated graphics (Intel Iris Xe / AMD Vega class), and 4K on a discrete GPU.
- Under 5% CPU on a modern machine (no CPU-side per-pixel work).
- Survives audio device changes, sleep/wake, and window resizes without crashing or a black screen.
- Shows a clear on-screen message (not a crash) when capture fails or permission is denied.

**As built:** all visual work is in WGSL shaders, capture reconnects on errors, and capture failures are shown on screen. Measured:
- **Linux (RTX 3080, 180 Hz):** Bars & Peaks uses 6.9% of one core at 180 fps (about 2.3% at 60 fps). MilkDrop uses about 45% of one core at 180 fps (about 15% at 60), mostly projectM evaluating each preset's equations on the CPU, which is inherent to MilkDrop.
- **Windows (an older Windows 11 laptop):** holds 60 fps with MilkDrop at about 11–12% CPU. Full screen jittered slightly while Windows Update was saturating the CPU. The frame latency was raised from 1 to 2 frames to absorb that, and the detailed FPS counter shows any remaining late frames.

## 10. Stretch goals (post-v1)

- **macOS support.** Primary: a **Core Audio process tap** (macOS 14.2+) tapping all system output. Fallback: **ScreenCaptureKit** audio-only capture (macOS 13+). Last resort: the user installs a virtual loopback device (e.g. BlackHole) and picks it as input. Expect a permission prompt, and handle a denial with a clear on-screen message. Needs a small Objective-C/Swift shim or `objc2` bindings, an unsigned `.app` bundle, and a third CI target. Because rendering uses wgpu (Metal on macOS), the rest of the app should run unchanged.
- "Now playing" overlay via OS media APIs (Windows SMTC, Linux MPRIS, macOS MediaRemote/Now Playing).
- Hot-reload of WGSL shaders from a user folder, so new visualizers can be written without recompiling.
- A screensaver mode (Windows `.scr`).
- Recording or exporting a clip to video.
- Album-art-tinted colour palettes.

## 11. Project structure (as built)

```
neoviz/
├── Cargo.toml
├── src/
│   ├── main.rs              # CLI (clap), startup
│   ├── app.rs               # winit event loop, window creation
│   ├── app/
│   │   ├── session.rs       # app state, switching, settings changes, span mode
│   │   ├── layout.rs        # monitor layout for span mode
│   │   ├── awake.rs         # keep-screen-on policy
│   │   ├── fade.rs          # fade on silence
│   │   ├── frame_stats.rs   # FPS counter figures
│   │   └── icon.rs          # window icon and app ID
│   ├── config.rs            # Settings, TOML persistence
│   ├── input.rs             # key bindings
│   ├── logging.rs           # stderr + neoviz.log
│   ├── audio/
│   │   ├── mod.rs           # capture thread, ring buffer, reconnect
│   │   ├── windows.rs       # WASAPI loopback (cpal)
│   │   ├── linux.rs         # PulseAudio monitor source
│   │   ├── analysis.rs      # FFT, levels, silence → AudioFrame
│   │   ├── bands.rs         # log-spaced bands, smoothing, peaks
│   │   └── beat.rs          # onset detection
│   ├── render/
│   │   ├── gpu.rs           # wgpu setup
│   │   ├── compositor.rs    # crossfade, bloom, scanlines
│   │   ├── feedback.rs      # feedback buffer
│   │   ├── layer.rs, lines.rs, streaks.rs, present.rs, common.rs
│   │   ├── debug_view.rs    # `D` analysis view
│   │   └── headless_gl/     # hidden GL context for projectM: mod.rs, egl.rs, wgl.rs
│   ├── viz/
│   │   ├── mod.rs           # Visualizer trait, registry, Switcher, AutoSwitch
│   │   ├── bars.rs, scope.rs, particles.rs, lightshow.rs, starfield.rs
│   │   ├── layered.rs       # Ambient Swirl, Battery Rings
│   │   ├── color.rs, common.rs
│   │   └── milkdrop/        # mod.rs, deck.rs (instance + texture), worker.rs (GL thread), glew.rs, presets.rs
│   └── ui/                  # egui overlay, HUD, fps_hud.rs
├── shaders/*.wgsl (+ post/)
├── assets/icon/             # generated window icon (.png) and Windows .ico
├── build.rs                 # cfg(milkdrop), Windows .exe icon
├── packaging/linux/         # install.sh, .desktop, icon
├── packaging/windows/       # neoviz.rc (.exe icon)
├── packaging/make-icons.sh  # all icon files from one source
├── vendor/                  # patched projectm-sys and projectm
└── .github/workflows/       # ci.yml, release.yml
```

## 12. Milestones

1. ✅ **Skeleton**: window, wgpu, full-screen toggle, CI on Windows and Linux (`d34334c`).
2. ✅ **Audio in**: Windows and Linux loopback, debug view (`6a38ec3`).
3. ✅ **Analysis**: bands, smoothing, beat and silence detection (`6538c82`).
4. ✅ **First two visualizers**, switching and crossfade (`bf536c3`).
5. ✅ **Remaining built-in visualizers**, feedback buffer, bloom and scanlines (`431fb76`).
6. ✅ **Settings overlay, persistence and random mode** (`937e662`).
7. ✅ **projectM / MilkDrop**: Linux (`3d9f2a9`, then fixes through `9926998`), Windows (`f97718f`, on branch `milkdrop-windows`), then preloading and crossfades.
8. ✅ **Packaging**: Linux tarball with `install.sh` and a desktop entry; Windows `.exe` zip; tag-triggered release workflow (`12fb3ed`). **v0.9.0** released 2026-11-13.
9. *(Later)* **macOS**: see §10.

## 13. Acceptance criteria for v1

- [x] Play music in any app on Windows and Linux, launch the visualizer, and it reacts within a second with no configuration. *(Default-output loopback on both. Needs a manual pass on Windows, see §15.)*
- [x] At least 5 built-in visualizers, switchable with the keyboard, with crossfades. *(7, plus 4 variants.)*
- [x] Full-screen and windowed modes both work, including on a second monitor. *(`--monitor N` and span mode.)*
- [x] Settings persist across restarts.
- [x] Unplugging headphones or changing the default output device doesn't kill the visuals. *(Reconnect logic on both backends. Needs a manual pass on Windows, see §15.)*
- [x] A README covers build steps for Windows and Linux.
- [x] MilkDrop presets play on Windows and Linux, with the same controls and overlay options. *(Windows confirmed on a Windows 11 laptop.)*

## 14. Notes for the implementer

- **Do the audio capture early.** It's the riskiest part.
- Don't build anything macOS-specific yet, but avoid choices that would block it later (e.g. no Windows-only or Linux-only rendering or UI code outside the audio backends). *(MilkDrop's hidden GL context is the one other per-OS piece. It's behind `render/headless_gl/`, with one small backend per OS.)*
- Prefer GPU shaders for all visual work, and keep visualizer code small and self-contained so adding a new one is a single file plus a shader.
- Keep dependencies modest. Ask before pulling in a large framework (game engine, Electron, etc.).

## 15. Remaining for 1.0

Done since v0.9.0: MilkDrop on Windows (patched `projectm-sys`, WGL pbuffer, GLEW), ARM64 builds, the log file, preloading with crossfades, the silence fade, `Space` locking MilkDrop's timer, 6 dB sensitivity steps, the detailed FPS counter, and icon plumbing.

1. **App icon:** waiting on the final artwork. Then run `packaging/make-icons.sh` and check it on Windows (title bar, taskbar, `.exe` in Explorer) and on Linux after `install.sh` (Wayland takes it from the installed desktop entry).
2. **Windows check on the laptop**, once Windows Update has settled:
   - full-screen smoothness with the detailed FPS counter (late frames should be about zero)
   - unplugging headphones / changing the default device
   - sleep/wake, keep-awake, fade on silence
   - a second monitor and span mode, if one's handy
3. **Merge `milkdrop-windows` into `main`.** Opening the PR also runs CI's Windows lint and tests, which haven't run on this branch (only release builds have).
4. **Release 1.0.0:** bump `version` in `Cargo.toml`, tag `v1.0.0`, and let the release workflow publish it.

**Decide before making the repo or its releases public:** the packages link projectM (LGPL) statically. While the repo is private and the builds are for personal use, that's fine. If they're published, the LGPL's relinking requirement is met by keeping neoViz's full source (including `vendor/`) available alongside, and the packages should then include projectM's licence text.

**Not tested on hardware:** the ARM64 packages (Linux and Windows) build in CI but haven't been run.
