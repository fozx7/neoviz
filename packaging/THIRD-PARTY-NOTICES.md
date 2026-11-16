# Third-party notices

neoViz itself is MIT-licensed (see `LICENSE`). Its packages also contain:

## projectM (libprojectM 4.1)

MilkDrop support uses [projectM](https://github.com/projectM-visualizer/projectm), which is linked statically into the `neoviz` binary. projectM is licensed under the **GNU Lesser General Public License 2.1**; the full text is in `LICENSE-LGPL-2.1.txt`.

- projectM's source: https://github.com/projectM-visualizer/projectm (version 4.1.3, as bundled in the `projectm-sys` 1.2.2 crate).
- neoViz's complete source, including the slightly patched `projectm-sys` and `projectm` crates it builds projectM through (in `vendor/`), is available from the neoViz repository. To use a modified projectM, change the copy in `vendor/projectm-sys/libprojectM` and rebuild with `cargo build --release`.

## GLEW (Windows packages only)

The Windows build of projectM uses [GLEW](https://github.com/nigels-com/glew), linked statically. GLEW is available under the Modified BSD License, the Mesa 3-D License (MIT) and the Khronos License (MIT); see https://github.com/nigels-com/glew/blob/master/LICENSE.txt.

## The full COSMOS field (Event Horizon's background)

The Event Horizon visualizer draws a resized copy of *The full COSMOS field (UltraVISTA)*, taken with ESO's VISTA telescope, behind its black hole, warped by the simulated gravitational lensing.

- Credit: **ESO/UltraVISTA team. Acknowledgement: TERAPIX/CNRS/INSU/CASU**. The credit is shown on screen while Event Horizon is showing.
- Source: https://esahubble.org/images/heic1313b/
- Licence: [Creative Commons Attribution 4.0 International](https://creativecommons.org/licenses/by/4.0/), per https://esahubble.org/copyright/. The copy in `assets/sky/` is resized from 17121×10824 to 4096×2590, and neoViz darkens its blacks and adds contrast.

## Rust crates

The Rust libraries neoViz is built from are under permissive licences (mostly MIT and/or Apache-2.0). `cargo tree` in the source lists them.
