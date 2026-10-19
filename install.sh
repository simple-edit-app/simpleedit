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

# Public half of the release signing key (Ed25519); keep in sync with
# SIGNING_PUBLIC_KEY in src/updater.rs.
SIGNING_PUBKEY='-----BEGIN PUBLIC KEY-----
MCowBQYDK2VwAyEAmZrvmo6ZzYbZD25u7dMsx6dm9HcyrW/ijgwVb4vjeks=
-----END PUBLIC KEY-----'

# Verify a downloaded file against the release's SHA256SUMS, and that file's
# signature when it has one. Releases that predate the checksum or signature
# files are installed with a warning.
verify() {
    local file="$1" name="$2" work sums expected actual
    work=$(mktemp -d)
    if ! curl -fsSL "https://github.com/${REPO}/releases/download/${VERSION}/SHA256SUMS" -o "$work/SHA256SUMS" 2>/dev/null; then
        echo "! No SHA256SUMS published for ${VERSION}; skipping integrity check." >&2
        rm -rf "$work"
        return 0
    fi
    if curl -fsSL "https://github.com/${REPO}/releases/download/${VERSION}/SHA256SUMS.sig" -o "$work/SHA256SUMS.sig" 2>/dev/null; then
        if openssl pkeyutl -help 2>&1 | grep -q -- '-rawin'; then
            printf '%s\n' "$SIGNING_PUBKEY" > "$work/pub.pem"
            openssl pkeyutl -verify -pubin -inkey "$work/pub.pem" -rawin \
                -in "$work/SHA256SUMS" -sigfile "$work/SHA256SUMS.sig" >/dev/null 2>&1 \
                || { rm -rf "$work"; err "Invalid signature on SHA256SUMS. Aborting."; }
            ok "Signature verified"
        else
            echo "! This openssl can't verify Ed25519 signatures; checking the checksum only." >&2
        fi
    else
        echo "! No signature published for ${VERSION}; checking the checksum only." >&2
    fi
    expected=$(awk -v n="$name" '{f=$2; sub(/^\*/, "", f)} f==n {print $1; exit}' "$work/SHA256SUMS")
    rm -rf "$work"
    [ -n "$expected" ] || err "No checksum listed for ${name}."
    if command -v sha256sum &>/dev/null; then
        actual=$(sha256sum "$file" | awk '{print $1}')
    else
        actual=$(shasum -a 256 "$file" | awk '{print $1}')
    fi
    [ "$expected" = "$actual" ] || err "Checksum mismatch for ${name}. Aborting."
    ok "Checksum verified (${name})"
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
            verify "$TMP" "$DEB"
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
            curl -fsSL --progress-bar "$URL" -o "$TMP/$TARBALL"
            verify "$TMP/$TARBALL" "$TARBALL"
            tar -xz -C "$TMP" -f "$TMP/$TARBALL"
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
            # Use the fully-qualified cask name so Homebrew auto-taps and
            # auto-trusts this one cask non-interactively (tapping first,
            # then installing by short name, hits the interactive tap-trust
            # prompt added in Homebrew 6.0 and fails under `curl | bash`).
            brew install --cask simple-edit-app/tap/simpleedit
        else
            # Manual install: extract binary to /usr/local/bin
            TARBALL="simpleedit-${VERSION}-aarch64-apple-darwin.tar.gz"
            URL="https://github.com/${REPO}/releases/download/${VERSION}/${TARBALL}"
            TMP=$(mktemp -d)
            info "Downloading ${TARBALL}…"
            curl -fsSL --progress-bar "$URL" -o "$TMP/$TARBALL"
            verify "$TMP/$TARBALL" "$TARBALL"
            tar -xz -C "$TMP" -f "$TMP/$TARBALL"
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
