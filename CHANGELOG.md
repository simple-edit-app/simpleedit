# Changelog

## [1.0.0] - 2026-11-02

### Added
- First release of SimpleEdit: a fast, stable, cross-platform text editor for macOS (Apple Silicon) and Linux (x86_64), built in Rust.
- Editing: syntax highlighting for 30+ languages, search and replace with regular expressions, line operations, code folding, bookmarks, Markdown preview, code formatting through external tools, dark and light themes, English and French, and a session that keeps unsaved text between launches.
- Go to definition: Ctrl+click (Cmd+click on macOS) on a function, variable, class, interface… jumps to where it is defined, and on a definition it lists where it is used. It only answers when it is sure; it never takes you somewhere wrong.
- Back / Forward (Alt+Left / Alt+Right, the mouse buttons, the menu and the command palette) walk the places you jumped from, across tabs.
- Language servers, optional: Preferences > Language servers installs and removes the one of a language (Rust, Python, TypeScript / JavaScript including React and Next.js, Go, Shell, PHP, C#) without administrator rights, and the status bar offers it when a file opens. Without a server, definitions are found by pattern search.
