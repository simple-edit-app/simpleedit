#!/usr/bin/env bash
set -euo pipefail

# ── helpers ──────────────────────────────────────────────────────────────────
info()  { echo "  $*"; }
ok()    { echo "✓ $*"; }
err()   { echo "✗ $*" >&2; exit 1; }

REMOVED=0
REFRESH_SYS=0
REFRESH_USER=0
DATA_HINT=""

# ── platform detection ───────────────────────────────────────────────────────
OS=$(uname -s)

case "$OS" in
    Linux)
        # .deb / apt package (also catches the leftover "config-files" state
        # left by older versions of this script that used `remove`, not `purge`)
        if command -v dpkg &>/dev/null && dpkg -s simpleedit &>/dev/null; then
            info "Removing .deb package (requires sudo)…"
            sudo apt-get purge -y simpleedit 2>/dev/null || sudo dpkg -P simpleedit
            REMOVED=1
            REFRESH_SYS=1
        fi

        # snap package
        if command -v snap &>/dev/null && snap list simpleedit &>/dev/null; then
            info "Removing snap package…"
            sudo snap remove simpleedit
            REMOVED=1
        fi

        # Stray binaries. Both dirs sit ahead of /usr/bin on a typical PATH, so
        # a leftover here silently shadows the packaged binary after a reinstall.
        if [ -f /usr/local/bin/simpleedit ]; then
            info "Removing binary from /usr/local/bin (requires sudo)…"
            sudo rm -f /usr/local/bin/simpleedit
            REMOVED=1
        fi
        if [ -f "$HOME/.local/bin/simpleedit" ]; then
            info "Removing binary from ~/.local/bin…"
            rm -f "$HOME/.local/bin/simpleedit"
            REMOVED=1
        fi

        # AppImage and its desktop integration
        for f in "$HOME/.local/bin/simpleedit.AppImage" \
                 "$HOME/Applications/simpleedit.AppImage" \
                 "$HOME/.local/share/applications/simpleedit.desktop"; do
            if [ -e "$f" ]; then
                info "Removing ${f/#$HOME/\~}…"
                rm -f "$f"
                REMOVED=1
                REFRESH_USER=1
            fi
        done

        [ "$REMOVED" -eq 1 ] || err "SimpleEdit does not appear to be installed."

        # Refresh menu / icon caches so a stale launcher entry disappears now
        if [ "$REFRESH_SYS" -eq 1 ]; then
            sudo gtk-update-icon-cache -f -t /usr/share/icons/hicolor 2>/dev/null || true
            sudo update-desktop-database /usr/share/applications 2>/dev/null || true
        fi
        if [ "$REFRESH_USER" -eq 1 ]; then
            update-desktop-database "$HOME/.local/share/applications" 2>/dev/null || true
        fi

        DATA_HINT="rm -rf ~/.config/simpleedit ~/.local/share/simpleedit"
        ;;

    Darwin)
        if command -v brew &>/dev/null && brew list --cask simpleedit &>/dev/null; then
            info "Uninstalling the Homebrew cask…"
            brew uninstall --cask simpleedit
            brew untap simple-edit-app/tap &>/dev/null || true
            REMOVED=1
        fi
        if [ -d /Applications/SimpleEdit.app ]; then
            info "Removing /Applications/SimpleEdit.app…"
            rm -rf /Applications/SimpleEdit.app
            REMOVED=1
        fi
        if [ -f /usr/local/bin/simpleedit ]; then
            info "Removing binary from /usr/local/bin (requires sudo)…"
            sudo rm -f /usr/local/bin/simpleedit
            REMOVED=1
        fi
        if [ -f "$HOME/.local/bin/simpleedit" ]; then
            info "Removing binary from ~/.local/bin…"
            rm -f "$HOME/.local/bin/simpleedit"
            REMOVED=1
        fi
        [ "$REMOVED" -eq 1 ] || err "SimpleEdit does not appear to be installed."

        DATA_HINT="rm -rf ~/Library/Application\\ Support/simpleedit"
        ;;

    *)
        err "Unsupported OS: $OS"
        ;;
esac

# ── done ─────────────────────────────────────────────────────────────────────
ok "SimpleEdit uninstalled."
echo ""
info "Your settings and session were left untouched. To remove them too:"
info "  ${DATA_HINT}"
