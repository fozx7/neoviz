//! - Sets `cfg(milkdrop)` when the "milkdrop" feature is on and the OS has a
//!   MilkDrop backend (Linux, Windows), so code gates on one name.
//! - On Windows, embeds the app icon in the `.exe` (Explorer, pinned taskbar).

use std::env;

fn main() {
    println!("cargo::rustc-check-cfg=cfg(milkdrop)");
    println!("cargo::rerun-if-changed=build.rs");
    let os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let feature_on = env::var_os("CARGO_FEATURE_MILKDROP").is_some();
    if feature_on && matches!(os.as_str(), "linux" | "windows") {
        println!("cargo::rustc-cfg=milkdrop");
    }
    if os == "windows" {
        println!("cargo::rerun-if-changed=assets/icon/neoviz.ico");
        println!("cargo::rerun-if-changed=packaging/windows/neoviz.rc");
        embed_resource::compile("packaging/windows/neoviz.rc", embed_resource::NONE)
            .manifest_optional()
            .expect("embedding the Windows icon");
    }
}
