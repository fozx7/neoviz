# Vendored crates

## projectm-sys 1.2.2 (patched)

A copy of [projectm-sys](https://crates.io/crates/projectm-sys) 1.2.2 from crates.io (Rust bindings that build libprojectM 4.1 from source). It's used through `[patch.crates-io]` in the top-level `Cargo.toml`. The only file that differs from the published crate is `build.rs`, and the changes affect only Windows builds:

- projectM is always built in **Release**, so it uses the same C runtime (`/MD`) as Rust. A Debug build (`/MDd`) can't link with it.
- The CMake generator isn't hard-coded to **Visual Studio 2022**, so whichever version is installed (e.g. 2026) is used.
- projectM uses its own bundled **projectm-eval** (as it does on Linux), so vcpkg only has to supply GLEW.
- The static libraries are linked by their real names: projectM's CMake adds a `lib` prefix on Windows (`libprojectM-4.lib`), but upstream linked `projectM-4`.
- **GLEW** is linked. projectM needs it on Windows, but upstream never linked it. vcpkg installs it into the CMake build directory (`vcpkg_installed/<triplet>`).
- The vcpkg triplet follows the target CPU (`x64-windows-static-md` or `arm64-windows-static-md`), so ARM64 Windows builds too.

Windows builds need vcpkg with `VCPKG_INSTALLATION_ROOT` set, which GitHub's Windows runners provide, plus CMake and Visual Studio 2022 or later.

To update: copy the new release from `~/.cargo/registry/src/*/projectm-sys-<version>`, reapply the `build.rs` changes (marked by the comment at its top), and drop the patch entirely if upstream fixes the Windows build.

The crate is LGPL (see `LICENSE`), the same as projectM itself.
