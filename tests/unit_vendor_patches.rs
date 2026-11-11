//! The patches in `vendor/` are documented in `vendor/PATCHES.md`; this fails
//! when one of them is no longer applied (for example after an iced bump).

use std::fs;
use std::path::Path;

fn read(path: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path))
        .unwrap_or_else(|e| panic!("{path}: {e}"))
}

#[test]
fn iced_winit_patch_is_applied() {
    let lib = read("vendor/iced_winit/src/lib.rs");
    assert!(lib.contains("SimpleEdit patch"));
    assert!(lib.contains("with_default_menu(false)"));
}

#[test]
fn iced_graphics_patch_is_applied() {
    let editor = read("vendor/iced_graphics/src/text/editor.rs");
    assert!(editor.contains("pub fn set_hidden_lines"));
}

#[test]
fn patches_are_wired_and_documented() {
    let manifest = read("Cargo.toml");
    assert!(manifest.contains(r#"iced_winit = { path = "vendor/iced_winit" }"#));
    assert!(manifest.contains(r#"iced_graphics = { path = "vendor/iced_graphics" }"#));
    let doc = read("vendor/PATCHES.md");
    assert!(doc.contains("iced_winit") && doc.contains("iced_graphics"));
}
