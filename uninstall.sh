#!/usr/bin/env bash
set -euo pipefail

# ── helpers ──────────────────────────────────────────────────────────────────
info()  { echo "  $*"; }
ok()    { echo "✓ $*"; }
err()   { echo "✗ $*" >&2; exit 1; }

# ── platform detection ───────────────────────────────────────────────────────
OS=$(uname -s)

case "$OS" in
    Linux)
        if command -v dpkg &>/dev/null && dpkg -s simpleedit &>/dev/null; then
            info "Removing .deb package (requires sudo)…"
            sudo apt-get remove -y simpleedit 2>/dev/null || sudo dpkg -r simpleedit
        elif [ -f /usr/local/bin/simpleedit ]; then
            info "Removing binary from /usr/local/bin (requires sudo)…"
            sudo rm -f /usr/local/bin/simpleedit
        else
            err "SimpleEdit does not appear to be installed."
        fi
        ;;

    Darwin)
        REMOVED=0
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
        [ "$REMOVED" -eq 1 ] || err "SimpleEdit does not appear to be installed."
        ;;

    *)
        err "Unsupported OS: $OS"
        ;;
esac

# ── done ─────────────────────────────────────────────────────────────────────
ok "SimpleEdit uninstalled."
