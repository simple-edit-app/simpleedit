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

# Verification is mandatory: the checksum list must be signed with the release
# key, and the file must match the list. A release that lacks either, or a
# machine that can't check the signature, makes the install fail. `--insecure`
# (or SIMPLEEDIT_INSECURE=1) installs anyway, with a warning, for old releases.
INSECURE="${SIMPLEEDIT_INSECURE:-0}"
for arg in "$@"; do
    case "$arg" in
        --insecure) INSECURE=1 ;;
        *) err "Unknown option: $arg" ;;
    esac
done

# An openssl that can verify Ed25519 signatures with -rawin (OpenSSL 3; the
# LibreSSL of macOS can't, Homebrew's openssl@3 can).
find_openssl() {
    local candidate
    for candidate in openssl /opt/homebrew/opt/openssl@3/bin/openssl /usr/local/opt/openssl@3/bin/openssl; do
        if command -v "$candidate" &>/dev/null && "$candidate" pkeyutl -help 2>&1 | grep -q -- '-rawin'; then
            echo "$candidate"
            return 0
        fi
    done
    return 1
}

# Either refuse (default) or warn (--insecure).
unverifiable() {
    if [ "$INSECURE" = "1" ]; then
        echo "! $1 Installing without that check (--insecure)." >&2
    else
        err "$1 Aborting; use --insecure to install anyway."
    fi
}

verify() {
    local file="$1" name="$2" work expected actual openssl
    work=$(mktemp -d)
    trap 'rm -rf "$work"' RETURN
    if ! curl -fsSL "https://github.com/${REPO}/releases/download/${VERSION}/SHA256SUMS" -o "$work/SHA256SUMS" 2>/dev/null; then
        unverifiable "No SHA256SUMS is published for ${VERSION}."
        return 0
    fi
    if ! curl -fsSL "https://github.com/${REPO}/releases/download/${VERSION}/SHA256SUMS.sig" -o "$work/SHA256SUMS.sig" 2>/dev/null; then
        unverifiable "No signature is published for ${VERSION}."
    elif ! openssl=$(find_openssl); then
        unverifiable "This system has no openssl able to verify Ed25519 signatures (install OpenSSL 3, e.g. 'brew install openssl@3')."
    else
        printf '%s\n' "$SIGNING_PUBKEY" > "$work/pub.pem"
        "$openssl" pkeyutl -verify -pubin -inkey "$work/pub.pem" -rawin \
            -in "$work/SHA256SUMS" -sigfile "$work/SHA256SUMS.sig" >/dev/null 2>&1 \
            || err "Invalid signature on SHA256SUMS. Aborting."
        ok "Signature verified"
    fi
    expected=$(awk -v n="$name" '{f=$2; sub(/^\*/, "", f)} f==n {print $1; exit}' "$work/SHA256SUMS")
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
            # apt also installs the package's dependencies; dpkg alone doesn't.
            if command -v apt-get &>/dev/null; then
                sudo apt-get install -y "$TMP"
            else
                sudo dpkg -i "$TMP"
            fi
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
