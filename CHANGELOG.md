# Changelog

## [1.5.0] - 2026-11-03

### Added
- Git panel (button with the branch icon under the file list, or its shortcut) with four tabs. **Commit**: staged and unstaged files, a commit message, Amend, Commit and Push, and the diff of the open file. **Stash**: save, apply, pop and drop. **Log**: the commit graph per branch with the files and message of a commit, copy hash, cherry-pick, revert, drop, squash and reword. **Branches**: local and remote branches and tags, to create, check out, merge, rebase, push, force push (with lease) and delete.
- Diffs open side by side by default, wrapped to the window, or unified (a button with a tooltip switches). Text can be selected by character (a double click picks a word) and copied (Ctrl+C, Ctrl+A, right click). Arrows bring a block of changes back, also for staged files. A diff can open in an editor tab, be compared with the current file, and be followed with Ctrl+Z.
- A Git submenu on a file (file list and editor right click): stage, commit this file, diff, compare with a branch, a tag or a revision, and the file's history.
- Changed lines are marked in the gutter and next to the scrollbar. A click on a bar shows a single arrow that puts the change back.
- Conflicts (rebase, merge, cherry-pick, revert): a pop-up at the top announces them with a button that settles them, and goes away by itself once they are settled or the operation is given up. Each conflicted file opens in three columns with an editable result, one-click resolution of a side, automatic resolution of the simple conflicts, undo and redo.
- Blame in the gutter, and a button to show the commit of a line in the log.
- Files are saved automatically a moment after the last edit (an option in the Preferences).

### Fixed
- Rolling back a staged rename brings the old name back; a removed line that starts with `-- ` stays in the diff; the last commit can be dropped; rewording the last commit no longer adds what is staged; the history of a file is drawn as one line.

## [1.4.0] - 2026-11-03

### Added
- Bookmarks panel: a button under the file list opens a panel with every bookmarked file and, under it, its bookmarked lines (`58: some code`). A double click opens the file, or the file at the chosen line. A file is added from the right click menu of the file list, a line from the right click menu of the editor (the cursor now goes to the line that was clicked). A right click in the panel deletes a bookmark.
- Bookmarks are shown as a yellow ribbon in the gutter and a yellow mark in the minimap.

## [1.3.0] - 2026-11-03

### Added
- Scratch files: a button under the file list opens a panel to create notes and trials outside any project: a name and an extension picked from an alphabetical list (or none). They are saved as you type, never close, stay out of the file list and last until you delete them (right click, Delete, with a confirmation). Their folder can be changed in the Preferences, and the files move along with it.

## [1.2.0] - 2026-11-02

### Added
- Integrated terminal: a `>_` button under the file list opens a terminal panel at the bottom of the window, with tabs to run several shells ("+" adds one, "×" closes one, "—" hides the panel). It starts in the project of the open file, supports colors, scrolling with the wheel, selection with the mouse, copy and paste (Ctrl+Shift+C / V, Cmd+C / V on macOS) and full-screen programs. Alt+F12 (rebindable), the View menu and the command palette toggle it.

## [1.1.0] - 2026-11-02

### Added
- A small cross beside each file in the sidebar closes its tab at once, without asking, even with unsaved changes.
- "Close other tabs" and "Close all tabs" in the File menu, the command palette and the right-click menu of a file in the sidebar (there, "others" keeps the file you clicked). One confirmation covers all the tabs that have unsaved changes.

### Fixed
- A click anywhere outside an open menu closes it, including on empty parts of the sidebar, the toolbar and the status bar.

## [1.0.1] - 2026-11-02

### Fixed
- Syntax highlighting no longer turns the rest of the file green after a template literal with `${…}` in JavaScript, TypeScript, JSX and TSX, and a JSX tag name that ends its line (`<button` followed by its attributes) is colored like its closing tag.
- JSX, TypeScript, TSX, Kotlin, Swift, SCSS, Less and TOML now have their own grammar instead of a close relative's, and JSX files no longer end inside a phantom regular expression.

## [1.0.0] - 2026-11-02

### Added
- First release of SimpleEdit: a fast, stable, cross-platform text editor for macOS (Apple Silicon) and Linux (x86_64), built in Rust.
- Editing: syntax highlighting for 30+ languages, search and replace with regular expressions, line operations, code folding, bookmarks, Markdown preview, code formatting through external tools, dark and light themes, English and French, and a session that keeps unsaved text between launches.
- Go to definition: Ctrl+click (Cmd+click on macOS) on a function, variable, class, interface… jumps to where it is defined, and on a definition it lists where it is used. It only answers when it is sure; it never takes you somewhere wrong.
- Back / Forward (Alt+Left / Alt+Right, the mouse buttons, the menu and the command palette) walk the places you jumped from, across tabs.
- Language servers, optional: Preferences > Language servers installs and removes the one of a language (Rust, Python, TypeScript / JavaScript including React and Next.js, Go, Shell, PHP, C#) without administrator rights, and the status bar offers it when a file opens. Without a server, definitions are found by pattern search.
