#!/usr/bin/env bash
set -euo pipefail

REPO="simple-edit-app/simpleedit"

# ── helpers ──────────────────────────────────────────────────────────────────
info()  { echo "  $*"; }
ok()    { echo "✓ $*"; }
err()   { echo "✗ $*" >&2; exit 1; }

need() {
    command -v "$1" &>/dev/null || err "Required tool not found: $1 — please install it and retry."
}

# ── resolve latest release ───────────────────────────────────────────────────
need curl

info "Fetching latest release…"
VERSION=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
    | grep '"tag_name"' | head -1 \
    | sed 's/.*"tag_name": *"\(.*\)".*/\1/')

[ -n "$VERSION" ] || err "Could not determine latest version. Check your internet connection."
info "Latest version: ${VERSION}"

# ── platform detection ───────────────────────────────────────────────────────
OS=$(uname -s)
ARCH=$(uname -m)

case "$OS" in
    Linux)
        case "$ARCH" in
            x86_64) ;;
            *) err "Unsupported architecture: $ARCH. Only x86_64 is supported on Linux." ;;
        esac

        # Prefer .deb on Debian/Ubuntu systems
        if command -v dpkg &>/dev/null; then
            # Derive exact .deb filename from the release assets
            DEB=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
                | grep '"name"' | grep '\.deb' | head -1 \
                | sed 's/.*"name": *"\(.*\)".*/\1/')
            [ -n "$DEB" ] || err "Could not find .deb asset in latest release."

            URL="https://github.com/${REPO}/releases/download/${VERSION}/${DEB}"
            TMP=$(mktemp /tmp/simpleedit-XXXXXX.deb)
            info "Downloading ${DEB}…"
            curl -fsSL --progress-bar "$URL" -o "$TMP"
            info "Installing (requires sudo)…"
            sudo dpkg -i "$TMP"
            rm -f "$TMP"
            # Refresh icon cache so the app appears in the launcher immediately
            sudo gtk-update-icon-cache -f -t /usr/share/icons/hicolor 2>/dev/null || true
            sudo update-desktop-database /usr/share/applications 2>/dev/null || true
        else
            # Generic Linux: extract binary to /usr/local/bin
            TARBALL="simpleedit-${VERSION}-x86_64-linux.tar.gz"
            URL="https://github.com/${REPO}/releases/download/${VERSION}/${TARBALL}"
            TMP=$(mktemp -d)
            info "Downloading ${TARBALL}…"
            curl -fsSL --progress-bar "$URL" | tar -xz -C "$TMP"
            info "Installing binary to /usr/local/bin (requires sudo)…"
            sudo mv "$TMP/simpleedit" /usr/local/bin/simpleedit
            sudo chmod +x /usr/local/bin/simpleedit
            rm -rf "$TMP"
        fi
        ;;

    Darwin)
        case "$ARCH" in
            arm64) ;;
            *) err "Only Apple Silicon (M-series) is supported on macOS." ;;
        esac

        if command -v brew &>/dev/null; then
            info "Installing via Homebrew…"
            brew tap simple-edit-app/tap 2>/dev/null || true
            brew install --cask simpleedit
        else
            # Manual install: extract binary to /usr/local/bin
            TARBALL="simpleedit-${VERSION}-aarch64-apple-darwin.tar.gz"
            URL="https://github.com/${REPO}/releases/download/${VERSION}/${TARBALL}"
            TMP=$(mktemp -d)
            info "Downloading ${TARBALL}…"
            curl -fsSL --progress-bar "$URL" | tar -xz -C "$TMP"
            info "Installing binary to /usr/local/bin (requires sudo)…"
            sudo mv "$TMP/simpleedit" /usr/local/bin/simpleedit
            sudo chmod +x /usr/local/bin/simpleedit
            rm -rf "$TMP"
        fi
        ;;

    *)
        err "Unsupported OS: $OS"
        ;;
esac

# ── done ─────────────────────────────────────────────────────────────────────
ok "SimpleEdit ${VERSION} installed successfully."
echo ""
echo "  Run:        simpleedit"
echo "  Open file:  simpleedit path/to/file"
echo ""
echo "  Uninstall:  curl -fsSL https://raw.githubusercontent.com/${REPO}/main/uninstall.sh | bash"
