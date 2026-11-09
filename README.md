# SimpleEdit

[![CI](https://github.com/simple-edit-app/simpleedit/actions/workflows/ci.yml/badge.svg)](https://github.com/simple-edit-app/simpleedit/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

A fast, stable, cross-platform text editor for **macOS** and **Linux**. Built in Rust with [iced](https://github.com/iced-rs/iced).

**[Website & Downloads](https://simple-edit-app.github.io/simpleedit/)**

---

## Features

- **Syntax highlighting** for 30+ languages (TextMate grammars)
- **Go to definition** — Ctrl+click (Cmd+click on macOS) on a function, variable, class or interface jumps to where it is defined; on the definition itself, it lists where it is used. It only answers when it is sure
- **Back / Forward** navigation (Alt+Left / Alt+Right, mouse buttons) across the places you jumped from, tabs included
- **Language servers, optional** — Preferences › Language servers installs (and removes) the one of a language without administrator rights; the status bar offers it when you open a file. Without one, definitions are found by a careful pattern search
- **Git** — a branch button under the file list opens a Git panel (Alt+G): *Commit* (changed files split into staged / not staged, side-by-side or unified diff with next / previous change and file, rollback, add, delete, jump to source, commit and push, amend), *Stash* (list, diff, apply, pop, delete) and *Log* (branch tree, commit graph, details, squash / drop / revert / cherry-pick / edit message, checkout, diff with the working tree), and *Annotate with Git Blame* from the editor's gutter. It needs `git` on your machine
- **Conflicts** — after a merge, rebase, cherry-pick or revert that stops on conflicts, the status bar and the Commit tab offer *Resolve conflicts*: for each file, keep yours, take theirs, or open a three-column merge (yours / result / theirs) with an arrow per chunk; *Continue* or *Abort* finishes the operation. In a side-by-side diff, a chevron carries a change from one side to the other
- **Auto-save and change markers** — modified files are written to disk a moment after you stop typing (Preferences › Save files automatically), and the editor's gutter and minimap mark the lines that changed since the last commit; in the Commit tab's diff, an arrow rolls one change back
- **Integrated terminal** — a `>_` button under the file list opens a terminal panel at the bottom, with tabs for several shells (Alt+F12, selection and copy / paste included)
- **Scratch files** — a `Scratch` button under the file list opens a panel to create notes and trials (name + extension); they are saved as you type, never close, and stay until you delete them. Their folder can be changed in the Preferences
- **Bookmarks panel** — a bookmark button under the file list opens a panel listing every bookmarked file with its bookmarked lines (`58: some code`); double-click a file or a line to open it (at the line). Add a file from the right click menu of the file list, a line from the right click menu of the editor; delete from the panel with a right click. Bookmarks show as a yellow ribbon in the gutter and a yellow mark in the minimap
- **Databases (PostgreSQL, MySQL / MariaDB, SQLite)** — a database button in the activity bar opens a panel of data sources, as in a JetBrains IDE. Each system has its own small driver program, downloaded by the app (checked by signature) the first time. PostgreSQL has everything below; **MySQL / MariaDB and SQLite** (a data source there is a host, or a file) open the tree, the tables (paged, sortable, filterable, editable by their primary key), the console with transactions, the exports, the definition of a table and the diagram; the table designer, the imports, the backups and the health are PostgreSQL's for now. A data source takes a URL or host / user / password, TLS (disable to verify-full), an SSH tunnel, `.pgpass`, read-only mode, a time zone, a startup script, a time limit for queries and a filter on schemas and tables; the password is kept in the system keychain when you choose *Forever*. The tree shows databases, schemas, tables (opened to their columns, keys and indexes), functions, sequences and types, with a search box. A table opens in a tab: paged, sortable, filterable with a `WHERE`, editable cell by cell (NULL, date pickers, add / delete rows, commit in one transaction, works without a primary key), columns you can resize, a value viewer for long text and JSON, copy of a cell or a row, and *follow the foreign keys* in both directions. A console at the bottom runs queries (selected, current or all; Stop button) with SQL completion and a log of everything the app runs. A right click on a table opens the designer (columns, types with `USING`, keys, constraints and indexes edited in place, schema, name, comments, with the SQL it will run), shows its DDL, empties it or drops it, always after a confirmation; a right click on a schema makes a new table or schema. PostgreSQL cannot move a column: new columns go at the end
- **Code folding** — chevrons in the gutter fold blocks, comments and Markdown sections
- **File sidebar** — open and switch between multiple files
- **Search & Replace** with regex support
- **Dark / Light theme**
- **Internationalisation** — English and French, switchable in Preferences and remembered across launches
- Configurable: font size, tab width, word wrap, auto-indent, bracket/quote completion
- Line editing: duplicate, move, comment/uncomment, indent/dedent
- Code formatting via external tools (prettier, rustfmt, …)
- Native binaries — macOS (Apple Silicon) and Linux (x86_64)

---

## Installation

### One-liner (Linux & macOS)

```bash
curl -fsSL https://raw.githubusercontent.com/simple-edit-app/simpleedit/main/install.sh | bash
```

Detects your OS automatically, downloads the latest release, and installs it.

**Uninstall:**
```bash
curl -fsSL https://raw.githubusercontent.com/simple-edit-app/simpleedit/main/uninstall.sh | bash
```

Detects how SimpleEdit was installed and removes it.

---

### macOS — Homebrew

Installs `SimpleEdit.app` into `/Applications` and puts `simpleedit` on your `PATH`:

```bash
brew install --cask simple-edit-app/tap/simpleedit
```

The app is signed ad-hoc, not notarised. If Gatekeeper blocks the first launch,
right-click the app and choose **Open**, or run:

```bash
xattr -dr com.apple.quarantine /Applications/SimpleEdit.app
```

**Uninstall:**
```bash
brew uninstall --cask simpleedit
# also drop your settings:
brew uninstall --zap --cask simpleedit
```

---

### Ubuntu / Debian — .deb

Download and install the latest `.deb` package:

```bash
curl -fsSL https://api.github.com/repos/simple-edit-app/simpleedit/releases/latest \
  | sed -n 's/.*"browser_download_url": *"\([^"]*\.deb\)".*/\1/p' \
  | xargs curl -fsSLO
sudo apt install ./simpleedit_*.deb && rm simpleedit_*.deb
```

This command does not verify the download. The [one-liner](#one-liner-linux--macos) does (Ed25519 signature on `SHA256SUMS`, then the file's checksum); to check by hand, download `SHA256SUMS` from the [Releases page](https://github.com/simple-edit-app/simpleedit/releases/latest) next to the `.deb` and run `sha256sum -c --ignore-missing SHA256SUMS`.

**Uninstall:**
```bash
sudo apt remove simpleedit
# or
sudo dpkg -r simpleedit
```

---

### Linux — tar.gz (any distro)

```bash
VERSION=$(curl -fsSL https://api.github.com/repos/simple-edit-app/simpleedit/releases/latest | grep '"tag_name"' | head -1 | sed 's/.*"\(.*\)".*/\1/')
curl -fsSL "https://github.com/simple-edit-app/simpleedit/releases/download/${VERSION}/simpleedit-${VERSION}-x86_64-linux.tar.gz" | tar xz
sudo mv simpleedit /usr/local/bin/
```

**Uninstall:**
```bash
sudo rm /usr/local/bin/simpleedit
```

---

### Build from source

```bash
# Prerequisites (Ubuntu/Debian)
sudo apt-get install libgtk-3-dev libxkbcommon-dev

# Build & run
cargo build --release
./target/release/simpleedit
```

---

## Usage

```bash
simpleedit                  # open with last session
simpleedit path/to/file     # open a specific file (several files: one tab each)
simpleedit new-file.txt     # a file that doesn't exist yet opens empty; saving creates it
```

### Good to know

- Files up to **64 MB** open; larger ones are refused. Files must be **UTF-8** text. Line endings (LF / CRLF) and the presence of a final newline are kept as they were.
- **Unsaved text is kept between runs**: the session (including the content of untitled and modified buffers) is stored in `~/.local/share/simpleedit/session.json` (`~/Library/Application Support/simpleedit` on macOS), readable by you only. Delete that folder to forget it.
- **Language servers** installed from the Preferences live in `~/.local/share/simpleedit/lsp` (`~/Library/Application Support/simpleedit/lsp` on macOS). They need their installer tool on your machine: `npm` (Python, TypeScript / JavaScript, Shell, PHP), `rustup` (Rust), `go` (Go) or `dotnet` (C#). For the others (C / C++, Java, Lua, Ruby, Zig, Kotlin, Swift) the Preferences show the command to run.
- Formatters (prettier, rustfmt, black…) run in the folder of the file, so the project's own configuration applies. prettier must be installed; nothing is downloaded for you.

---

## Development

```bash
cargo run           # run in dev mode
cargo test          # run tests
cargo clippy        # lint
cargo fmt           # format source
```

---

## License

MIT — see [LICENSE](LICENSE).
