#!/usr/bin/env bash
# Hiveloom release builder + uploader.
#
# Builds the CLI for each supported target, strips the binaries, generates
# SHA-256 checksums, and uploads the result to a Cloudflare R2 bucket
# under /releases/<version>/ and mirrors to /releases/latest/.
#
# Usage:
#   scripts/release.sh                          # build + upload (version from Cargo.toml)
#   scripts/release.sh --version 0.2.0          # override version
#   scripts/release.sh --skip-upload            # build only
#   scripts/release.sh --skip-build             # upload an already-built dist/<version>
#   scripts/release.sh --targets linux-x86_64,linux-aarch64
#   scripts/release.sh --no-latest              # don't mirror to /releases/latest/
#   scripts/release.sh --no-install-sh          # don't re-upload install.sh
#
# Environment:
#   RCLONE_REMOTE   rclone remote name (default: r2)
#   R2_BUCKET       bucket name (default: hiveloom-releases)

set -euo pipefail

# ── Locate repo root ─────────────────────────────────────────────────

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO_ROOT"

# ── Defaults ─────────────────────────────────────────────────────────

VERSION=""
SKIP_BUILD=0
SKIP_UPLOAD=0
MIRROR_LATEST=1
UPLOAD_INSTALL_SH=1
TARGETS_ARG=""
RCLONE_REMOTE="${RCLONE_REMOTE:-r2}"
R2_BUCKET="${R2_BUCKET:-hiveloom-releases-public}"

ALL_TARGETS=(
    "linux-x86_64:x86_64-unknown-linux-gnu"
    "linux-aarch64:aarch64-unknown-linux-gnu"
    "darwin-x86_64:x86_64-apple-darwin"
    "darwin-aarch64:aarch64-apple-darwin"
)

# ── Parse flags ──────────────────────────────────────────────────────

while [[ $# -gt 0 ]]; do
    case "$1" in
        --version)        VERSION="$2"; shift 2 ;;
        --skip-build)     SKIP_BUILD=1; shift ;;
        --skip-upload)    SKIP_UPLOAD=1; shift ;;
        --no-latest)      MIRROR_LATEST=0; shift ;;
        --no-install-sh)  UPLOAD_INSTALL_SH=0; shift ;;
        --targets)        TARGETS_ARG="$2"; shift 2 ;;
        -h|--help)
            sed -n '2,20p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            echo "Unknown flag: $1" >&2
            exit 1
            ;;
    esac
done

# ── Version ──────────────────────────────────────────────────────────

if [[ -z "$VERSION" ]]; then
    VERSION="$(grep -m1 -E '^version = ' Cargo.toml | sed -E 's/version = "(.*)"/\1/')"
fi
if [[ -z "$VERSION" ]]; then
    echo "Could not determine version. Pass --version or fix Cargo.toml." >&2
    exit 1
fi

DIST_DIR="dist/$VERSION"
echo "▸ Hiveloom release v$VERSION → $DIST_DIR"

# ── Pick targets ─────────────────────────────────────────────────────

HOST_OS="$(uname -s)"
SELECTED_TARGETS=()

if [[ -n "$TARGETS_ARG" ]]; then
    IFS=',' read -r -a requested <<< "$TARGETS_ARG"
    for want in "${requested[@]}"; do
        for entry in "${ALL_TARGETS[@]}"; do
            if [[ "${entry%%:*}" == "$want" ]]; then
                SELECTED_TARGETS+=("$entry")
                continue 2
            fi
        done
        echo "Unknown target: $want" >&2
        exit 1
    done
else
    # Default: Linux targets everywhere. Darwin targets only on macOS (no
    # practical cross-compile from Linux — missing SDK + linker).
    for entry in "${ALL_TARGETS[@]}"; do
        label="${entry%%:*}"
        case "$label" in
            linux-*)
                SELECTED_TARGETS+=("$entry")
                ;;
            darwin-*)
                if [[ "$HOST_OS" == "Darwin" ]]; then
                    SELECTED_TARGETS+=("$entry")
                else
                    echo "  skip $label (not on macOS)"
                fi
                ;;
        esac
    done
fi

echo "  targets: ${SELECTED_TARGETS[*]%%:*}"

# ── Build ────────────────────────────────────────────────────────────

build_target() {
    local label="$1" triple="$2"
    local host_triple builder strip_bin out

    echo ""
    echo "▸ build $label ($triple)"

    # Decide whether to use `cargo` or `cross`. `cross` runs builds inside a
    # Docker image that carries the right toolchain — required for
    # Linux→Linux cross with rusqlite's bundled C extension.
    host_triple="$(rustc -vV | awk '/^host: / {print $2}')"
    if [[ "$triple" == "$host_triple" ]] || [[ "$HOST_OS" == "Darwin" && "$triple" == *-apple-darwin ]]; then
        builder="cargo"
        # Native/mac: ensure std is present for this target on the host.
        rustup target add "$triple" >/dev/null 2>&1 || true
    else
        builder="cross"
        if ! command -v cross >/dev/null 2>&1; then
            echo "  'cross' not found — install with: cargo install cross --locked" >&2
            exit 1
        fi
        if ! docker info >/dev/null 2>&1; then
            echo "  docker daemon not reachable — cross needs docker running" >&2
            exit 1
        fi
        # Cross builds inside Docker and manages its own toolchain there —
        # do NOT `rustup target add` on the host. On rustup 1.28+ that
        # tries to pull a full cross toolchain and fails on non-x86 hosts.
    fi

    "$builder" build --release --locked --target "$triple"

    out="target/$triple/release/hiveloom"
    if [[ ! -f "$out" ]]; then
        echo "  build produced no binary at $out" >&2
        exit 1
    fi

    mkdir -p "$DIST_DIR"
    local dest="$DIST_DIR/hiveloom-$label"
    cp "$out" "$dest"

    # Strip — platform-appropriate binary. Skip darwin binaries if not on mac.
    if [[ "$label" == linux-* ]]; then
        case "$triple" in
            x86_64-unknown-linux-gnu)  strip_bin="strip" ;;
            aarch64-unknown-linux-gnu) strip_bin="aarch64-linux-gnu-strip" ;;
        esac
        if command -v "$strip_bin" >/dev/null 2>&1; then
            "$strip_bin" "$dest" || true
        fi
    elif [[ "$HOST_OS" == "Darwin" && "$label" == darwin-* ]]; then
        strip "$dest" || true
    fi

    ( cd "$DIST_DIR" && sha256sum "hiveloom-$label" > "hiveloom-$label.sha256" )
    echo "  → $dest ($(du -h "$dest" | cut -f1))"
}

if [[ "$SKIP_BUILD" == "0" ]]; then
    for entry in "${SELECTED_TARGETS[@]}"; do
        build_target "${entry%%:*}" "${entry##*:}"
    done
else
    echo "▸ skip build (using existing $DIST_DIR)"
    if [[ ! -d "$DIST_DIR" ]]; then
        echo "  $DIST_DIR does not exist" >&2
        exit 1
    fi
fi

# ── Upload to R2 ─────────────────────────────────────────────────────

if [[ "$SKIP_UPLOAD" == "1" ]]; then
    echo ""
    echo "▸ skip upload — artifacts are in $DIST_DIR"
    exit 0
fi

if ! command -v rclone >/dev/null 2>&1; then
    echo "rclone not found. Install it or run with --skip-upload." >&2
    exit 1
fi

if ! rclone listremotes | grep -q "^${RCLONE_REMOTE}:$"; then
    echo "rclone remote '${RCLONE_REMOTE}:' is not configured." >&2
    echo "See scripts/install.sh header for setup, or run 'rclone config'." >&2
    exit 1
fi

VERSIONED_DEST="${RCLONE_REMOTE}:${R2_BUCKET}/releases/${VERSION}/"
LATEST_DEST="${RCLONE_REMOTE}:${R2_BUCKET}/releases/latest/"

echo ""
echo "▸ upload → $VERSIONED_DEST"
rclone copy "$DIST_DIR" "$VERSIONED_DEST" --progress

if [[ "$MIRROR_LATEST" == "1" ]]; then
    echo ""
    echo "▸ mirror → $LATEST_DEST"
    rclone copy "$DIST_DIR" "$LATEST_DEST" --progress
fi

if [[ "$UPLOAD_INSTALL_SH" == "1" ]]; then
    echo ""
    echo "▸ upload install.sh → bucket root"
    # Stage in a temp dir and use `rclone copy` — rclone's copyto/rcat single-file
    # uploads intermittently 403 against R2 even with correct token scope.
    stage_dir="$(mktemp -d)"
    trap 'rm -rf "$stage_dir"' EXIT
    cp "$SCRIPT_DIR/install.sh" "$stage_dir/install.sh"
    rclone copy "$stage_dir" "${RCLONE_REMOTE}:${R2_BUCKET}/" --include "install.sh"
fi

# Note: if you need short cache on /releases/latest/ or install.sh, configure
# it in Cloudflare (bucket-level Cache-Control override or a Transform Rule)
# rather than via rclone's --header-upload, which R2 can reject at sign time.

echo ""
echo "✓ Released v$VERSION"
echo ""
echo "Verify:"
echo "  curl -fsSL https://bin.hiveloom.cloud/install.sh | bash"
echo "  curl -fsI https://bin.hiveloom.cloud/releases/${VERSION}/hiveloom-linux-aarch64"
