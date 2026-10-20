# Changelog

## [1.12.1] - 2026-10-20

### Fixed
- Launched from the desktop menu, the window now confirms the launch to GNOME, so the "loading" mouse cursor no longer lingers after the app has opened.

## [1.12.0] - 2026-10-20

### Added
- Code folding: a chevron in the gutter folds or unfolds blocks between braces (functions, JSON objects and arrays), comment blocks, indented blocks (Python, YAML, HTML…), Markdown sections and code fences. Ctrl+click also folds the blocks of the same level, Alt+click everything inside; a tooltip on the chevron lists the keys.
- "Fold / Unfold Block" (Ctrl+Shift+L, rebindable), "Fold All" and "Unfold All" in the Edit menu and the command palette. Arrow keys skip folded lines; typing in a folded block, a search, "Go to line" or a bookmark opens it.

## [1.11.0] - 2026-10-20

### Added
- The File menu keeps the recent files in a "Recent Files" submenu that opens on hover, with a "Clear" entry to empty the list.

## [1.10.0] - 2026-10-20

### Fixed
- Windows (CRLF) files are no longer doubled when saved, and a missing final newline is kept as it was.
- Tabs are no longer lost: Tab types a real tab in tab mode, and trailing tabs survive paste, undo and format.
- A format result can no longer land in another tab or overwrite what you typed meanwhile.
- A file changed on disk while it was in a background tab is now noticed before you save over it.
- Relative paths given on the command line are resolved, so the session can't save into the wrong file; several files can be opened at once.
- A damaged session file is set aside instead of being overwritten, and quitting warns if the session could not be saved.
- Closing the last tab leaves a tab to type in; "Save As" onto an open tab with unsaved work is refused.
- Alt+Up / Alt+Down move the line, and shortcuts keep working while the search field has the focus.
- Search selects the right text after emoji, accents and right-to-left text; "Replace all" no longer drops a final blank line.
- Undo, redo and line commands keep the cursor, the selection and the bookmarks; going back to the saved text clears the modified mark.
- Comment toggling no longer breaks doc comments and shebangs; Kotlin, TSX and JSX format correctly.
- Names in Chinese, Japanese, Arabic or with emoji display properly in the interface.

### Added
- Syntax highlighting for TypeScript, TSX, JSX, TOML, Swift and Kotlin, and more file types recognized by name or shebang.
- Preference to turn word wrap off (faster on very long lines), and a hint to format a file that has one.
- The command palette ignores accents: "pref" finds "Préférences".
- Each tab remembers its cursor and scroll position.

### Changed
- Much faster on large files: jumping to a line, undo, line commands, search, the autosave and the undo history.
- The command-line installer and updater check signatures strictly; Prettier is no longer downloaded behind your back; formatters run in the file's folder so project settings apply.
- Built on iced 0.14, with updated dependencies fixing known security advisories.

## [1.9.0] - 2026-10-19

### Added
- Changelog, in English and French: open it from Help, "What's new...".
- The update dialog lists what the new version changes, and SimpleEdit shows what's new the first time it runs after an update.

## [1.8.0] - 2026-10-19

### Added
- Bookmarks: mark a line to find it again. Toggle with Ctrl+F2, jump with F2 / Shift+F2, or use the Edit menu and the right-click menu (add, remove, remove all).
- Bookmarks show as a flag in the gutter, and as a blue line in the minimap.
- Bookmarks are saved per file and come back when you reopen the file or the app, and they follow the text when lines are added or removed.
- Right-click now moves the caret to the clicked line first.

## [1.7.0] - 2026-10-19

### Added
- Minimap on the right edge: click or drag to move around the file, with the visible area framed in orange and search matches marked.
- Option to hide the minimap in Preferences.

### Fixed
- Long folder paths no longer push the Format button out of the toolbar.
- Clicking far away in a large file no longer freezes development builds.

## [1.6.0] - 2026-10-19

### Changed
- New "Nocturne" design: theme, fonts, floating panels, command palette.

### Fixed
- The line-number gutter stays aligned with wrapped lines and scrolling.
- Data-loss, search, updater-security and menu-hover fixes from an audit.

## [1.5.0] - 2026-09-03

### Added
- Case-sensitive and regex toggle buttons in the search bar; matches are easier to see.

## [1.4.2] - 2026-09-03

### Fixed
- The Replace button crashed the app.

## [1.4.1] - 2026-09-03

### Added
- Replace-one button in the search bar.

### Fixed
- Syntax highlighting was invisible in some cases.

## [1.4.0] - 2026-09-03

### Added
- Real find, next and previous in the search bar.

## [1.3.4] - 2026-09-03

### Fixed
- Hovering a Markdown table blocked scrolling the page.

## [1.3.3] - 2026-09-03

### Changed
- Redesigned Markdown table preview; the preview no longer has a right-click menu.

## [1.3.2] - 2026-09-03

### Fixed
- Markdown tables rendered empty or collapsed.

## [1.3.1] - 2026-09-03

### Fixed
- The app did not restart after an update.
- Missing icon in the JSON tree.

## [1.3.0] - 2026-09-03

### Added
- Tables are rendered in the Markdown preview.

## [1.2.0] - 2026-09-03

### Added
- Clickable anchor links in the Markdown preview.

## [1.1.3] - 2026-09-03

### Fixed
- Menu dropdowns were misaligned whatever the language.

## [1.1.2] - 2026-09-03

### Fixed
- Top menu dropdowns opened misaligned to the right.

## [1.1.1] - 2026-09-03

### Fixed
- The top menu closed immediately after opening.

## [1.1.0] - 2026-09-03

### Added
- Markdown and JSON preview panes.

## [1.0.1] - 2026-08-31

### Fixed
- The uninstaller handles AppImage, snap and stray binaries.
- The macOS installer no longer triggers a Homebrew tap-trust prompt.

## [1.0.0] - 2026-08-20

### Added
- First release: tabbed text editor with syntax highlighting, search, formatting, sessions, themes and English/French interface.
