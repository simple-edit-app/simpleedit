# Patches applied to the vendored crates

Both crates are copies of iced 0.14.1 (see `.cargo_vcs_info.json`, upstream sha
`3997291f`), wired through `[patch.crates-io]` in `Cargo.toml`. The upstream
history is not kept here, so every change is listed below.

When bumping iced, re-apply each patch by hand: Cargo ignores `[patch]` with
only a warning when the version no longer matches, which would silently lose
the macOS quit protection and break the build of code folding.
`tests/unit_vendor_patches.rs` fails if a marker below disappears.

## `iced_winit`

- `src/lib.rs` (~l. 81, marker `SimpleEdit patch`): `with_default_menu(false)`,
  so winit's default macOS menu, whose Cmd+Q quits without saving the session,
  is not installed.
- `src/conversion.rs` (~l. 145): activation token forwarded to the window.

## `iced_graphics`

- `src/text/editor.rs` (~l. 27-60, 697-770): `Editor::set_hidden_lines`, the
  API used by `src/code_editor.rs` to hide folded lines.
