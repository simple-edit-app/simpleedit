# Changelog

## [1.14.0] - 2026-11-06

### Added
- Replace in files, as in IntelliJ IDEA: Ctrl+Shift+R, or the new Replace in this folder of the project tree. With a regular expression, `$1` and `${name}` give the groups, and `\l \u \L \U \E` change the case; Preserve case keeps the case of what is replaced. Each match shows its replacement before anything is written. Replace does one match, Replace all the others after a confirmation; files open in the editor are changed there (Ctrl+Z undoes it), others on disk (Undo the replacement).
- Results panel, like IntelliJ's Find tool window: Ctrl+Enter sends the matches to it, in tabs that can be pinned, by file or by folder. Delete leaves a match out, Insert takes it back, Ctrl+X removes it, Alt+L and Alt+D replace, Ctrl+F5 searches again, Ctrl+Alt+↑/↓ go through the matches, Alt+O exports them.

### Changed
- Find in files has been redone: whole words, a search on several lines, a file mask (`*.js, !test*.js`), in or except comments and string literals, a preview of the lines around each match, a pin, recent searches (Alt+↓). It looks in the project, a folder with or without its subfolders, or the open, recent, changed or current files.
- Find in files and search everywhere now find the files Git ignores one by one, such as a `.env`, but not the folders it ignores as a whole. In Find in files, Git-ignored files can be turned off.
- Search everywhere sends its results to the Results panel.

## [1.13.0] - 2026-11-05

### Added
- Search everywhere, as in IntelliJ IDEA: Shift twice opens one field that finds classes, files and folders, symbols, actions, settings and text, by tab (All, Classes, Files, Symbols, Actions, Text, and Git when turned on in Preferences) or all at once. With nothing typed it lists the recent files. Shift twice again (or Alt+N) includes non-project items: what Git ignores, the libraries. `name:line:col` opens a file at a place, `Type.name` finds a qualified name, `/` lists the settings (turned ON/OFF in place) and the pages of the Preferences, a calculation (`2^10`, `sqrt(2)`, `0xff + 1`) is worked out and copied with Enter. Actions are found by their words in any order, by synonyms and by abbreviations of one's own (right click on an action), and Alt+Enter sets an action's shortcut. Results can be filtered by file type and opened in Find in files. Tab and Shift+Tab change tabs, Ctrl+↑/↓ go to the first and last results.
- Shortcuts for the tabs: Ctrl+Shift+N (files), Ctrl+Alt+N (classes), Ctrl+Alt+Shift+N (symbols), Ctrl+Shift+A (actions).

### Changed
- Go to file and the command palette are now the Files and Actions tabs of search everywhere.

## [1.12.0] - 2026-11-05

### Added
- Code completion from the language servers (TypeScript/JavaScript, Rust, Python, Go, C/C++, Java and the others the editor knows). The list opens while typing a name or after `.`, `::`… (Ctrl+Space to ask), holds only what the server proposes, narrowed by what was typed, in the server's order, with each item's kind, type and documentation. Enter puts the item in, Tab replaces the rest of the word; imports are added with it (one undo step), and the places of a function's arguments are filled with Tab. A list the server says is incomplete is asked again while typing, and while the server is still loading the project the list says so and is asked again once it is done. Without a language server, nothing is proposed.
- Parameter info: the signature of the call being typed, the parameter at the cursor marked, opens on `(` and `,` (Ctrl+P to ask); Alt+↑/↓ go through the overloads.
- Preferences > Editor: completion on or off, while typing or on the shortcut only, and per language: imports from other modules (TypeScript/JavaScript, Rust, Python), postfix completions and call arguments (Rust).
- Language servers start when a file of their project is opened and load it in the background; only the changed part of a file is sent to them on each keystroke.

## [1.11.0] - 2026-11-05

### Changed
- A new look: the background is IntelliJ's dark grey, the panels (files, editor, bar of the panels) touch and are split by hairlines, as in Zed, and nothing is rounded any more (buttons, fields, menus, cards, highlights).
- The tabs follow Zed: a bar a little lighter than the editor, hairlines between the tabs, and the open tab in the editor's color, joined to it.

### Removed
- The "Format code" button above the editor: it is in the Edit menu, in the right click menu of the editor, in the palette and has its shortcut.

## [1.10.0] - 2026-11-05

### Added
- Colored icons before the files' names, by kind of file (Rust, TypeScript, JSON, Docker, npm, YAML, Markdown, images…), in the tree, the open files, the tabs, the Git, scratch and bookmark panels, Go to file and Find in files; folders get a thin outline. The icons come from the Seti set of the Nerd Font and from Lucide.

### Fixed
- A project reached through a symbolic link (and any project on macOS under `/var`) is colored by Git again in the tree.

## [1.9.0] - 2026-11-05

### Added
- Projects: folders added to the sidebar (File > Add a folder, or the `+` of the Projects panel) are shown as trees, read as they are opened and followed as the disk changes. From the tree: new file, new folder, rename, duplicate, delete (a folder after a question), move by dragging onto a folder, copy the path, open a terminal there, show in the file manager, find in this folder. The arrows move in the tree and Enter opens. Fold everything, hide what Git ignores, and names to leave out (`node_modules/, *.log`) in Preferences > Editor.
- A bar on the left opens the Files or the Projects panel (one at a time) and holds the buttons of the terminal, the scratch files, the bookmarks and Git; the bar under the file list is gone.
- Tabs above the editor: they scroll with the wheel, are reordered by dragging, close with the middle button, show the kind of file, and a list (`⌄`) holds them all. Ctrl+Tab goes to the tab seen before (further while Ctrl is held), Ctrl+Shift+T opens the closed tab again. A file opened from the tree takes a preview tab (in italics), kept once it is edited or clicked twice.
- Go to file (Ctrl+Shift+N): a file of the projects found by its name. Find in files (Ctrl+Shift+F): a text in every file of the projects, or of the folder picked in the tree, with case and regular expressions.
- Files are colored by how they stand in Git in the tree and on the tabs (changed, new, in conflict, ignored), and a red dot marks the files where the lint found errors.

### Fixed
- An open menu no longer closes by itself while the pointer goes down it.

## [1.8.1] - 2026-11-04

### Fixed
- The gap between the file list and the editor was twice as wide as the others; it now is one gap, and the whole gap grabs to resize the list.

## [1.8.0] - 2026-11-04

### Added
- Lint: problems found by linters and language servers are shown in the editor, with a card on hover that gives the message, the rule and the fixes. Off to begin with: you turn it on for each language in Preferences > Lint (a language can also be turned on from the right click menu of the editor).
- Tools found by themselves, never installed: the project's own (`node_modules/.bin`, `.venv`, `vendor/bin`), then the system's; a path can be chosen for each tool. ESLint, TypeScript (language server), Ruff, flake8, pycodestyle, pylint, mypy, Clippy, go vet, golangci-lint, cppcheck, ShellCheck, Stylelint, yamllint, markdownlint, RuboCop, php, luacheck, ktlint, SwiftLint, SQLFluff, HLint, xmllint, JSON, and the language servers of Rust, Go, C/C++, Java, C#, Ruby, PHP, Kotlin, Swift, Dart, Lua, Haskell and more.
- Fixes: one problem from its card, or all the problems of the same source (ESLint, TypeScript…) in the file; "Fix the problems of the file" from the Edit menu, the palette, the right click of the editor and of a file in the list. By default only errors are fixed (warnings, or everything, in the settings); fixes can also be applied when a file is saved.
- Each severity has its own marking (wavy line, plain line, highlight, or wavy line and highlight) and its own color. Problems also show in the minimap and in the status bar (`✖ 2 ⚠ 3`, a click goes to the next one); next and previous problem in the Edit menu and the palette.
- When the lint runs: as you type, when you save or only when asked, and a first look when a file is opened (an option). The status bar says when no tool was found, and what a failing tool complained about.
- The Preferences are in the File menu.

### Fixed
- The editor no longer loses its focus and its caret when a floating card opens or closes.

## [1.7.0] - 2026-11-04

### Added
- Split editor: the editor can be split as many times as you like, side by side or one above the other, and split again inside any pane. It is in the View menu, the command palette, the right click menu of the editor, and, to show a file in a new pane, the right click menu of the file list (Open to the right, Open below). Each pane has a header with its file and a cross to close it. A click in a pane gives it the focus; the wheel scrolls it without. Panes on the same file keep their own cursor and scroll while the text follows.

## [1.6.0] - 2026-11-04

### Added
- The file list can be resized by dragging its edge (its width is remembered) and shows the full path in a tooltip. Right click, Delete, removes a file for real, with a confirmation and a check box to remove it from Git as well when it is tracked.
- Changes of a versioned file are also marked in the minimap as a band, like bookmarks. In the gutter the cursor is an arrow, and a hand over the fold chevrons, the changes and the bookmarks. The blame shows an IntelliJ-style card after a short delay, and "previous version of this line" opens the file as it was before that commit.
- Keyboard shortcuts reviewed after IntelliJ (existing custom shortcuts are kept), and Ctrl+Shift+B for the list of branches.
- Git panel: Fetch (and an automatic fetch, set in the Preferences) with a notice of new commits; Pull and Merge as split buttons whose modes (pull, pull --rebase, fast-forward only, merge, squash...) are set in the Preferences; new branches and tags are named in a pop-up; tags can be pushed and deleted on origin; undo the last commit; stash a single file; add a file to .gitignore.
- Diffs: stage or unstage a block or the selected lines, changed words highlighted inside a line, a button to ignore whitespace, a button to change the context (3, 10, 30 lines, whole file), and syntax colors.
- Log: a right click menu to create a branch or a tag here, check out, reset (soft, mixed, hard) and compare; a search by message, author, added or removed text, file or date; and an interactive rebase (reorder, reword, edit, squash, fixup, drop).
- A branch switcher in the status bar, with a filter and branch creation.
- The panels at the bottom (Git, terminal, scratch files, bookmarks) can be resized by dragging their top edge. The buttons under the file list follow the width of the list, which cannot get narrower than they need.
- A scratch file made without a name is called `scratch1`, `scratch2`...

### Fixed
- A Git error is shown in the status bar when the Git panel is closed.

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
