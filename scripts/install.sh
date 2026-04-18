#!/usr/bin/env bash
# Hiveloom install script
# Usage:
#   curl -fsSL https://bin.hiveloom.cloud/install.sh | bash
#   curl -fsSL https://bin.hiveloom.cloud/install.sh | bash -s -- --version 0.2.0
#   curl -fsSL https://bin.hiveloom.cloud/install.sh | bash -s -- --install-dir ~/.local/bin

set -euo pipefail

VERSION="latest"
INSTALL_DIR="/usr/local/bin"
DATA_DIR="/var/lib/hiveloom"
SERVICE_USER="hiveloom"
BASE_URL="https://bin.hiveloom.cloud/releases"
INSTALL_SERVICE=1

# ── Parse flags ──────────────────────────────────────────────────────

while [[ $# -gt 0 ]]; do
    case "$1" in
        --version)       VERSION="$2"; shift 2 ;;
        --install-dir)   INSTALL_DIR="$2"; shift 2 ;;
        --no-service)    INSTALL_SERVICE=0; shift ;;
        -h|--help)
            sed -n '2,6p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            echo "Unknown flag: $1" >&2
            exit 1
            ;;
    esac
done

# ── Detect platform ──────────────────────────────────────────────────

detect_platform() {
    local os arch
    os="$(uname -s | tr '[:upper:]' '[:lower:]')"
    arch="$(uname -m)"

    case "$os" in
        linux)  os="linux" ;;
        darwin) os="darwin" ;;
        *)
            echo "Unsupported OS: $os" >&2
            exit 1
            ;;
    esac

    case "$arch" in
        x86_64|amd64)    arch="x86_64" ;;
        aarch64|arm64)   arch="aarch64" ;;
        *)
            echo "Unsupported architecture: $arch" >&2
            exit 1
            ;;
    esac

    echo "${os}-${arch}"
}

PLATFORM="$(detect_platform)"
echo "Detected platform: $PLATFORM"

# ── Download ─────────────────────────────────────────────────────────

BINARY_URL="${BASE_URL}/${VERSION}/hiveloom-${PLATFORM}"
echo "Downloading Hiveloom ${VERSION} from ${BINARY_URL}..."

TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

if command -v curl &>/dev/null; then
    curl -fsSL --retry 3 -o "${TMPDIR}/hiveloom" "$BINARY_URL"
elif command -v wget &>/dev/null; then
    wget -q -O "${TMPDIR}/hiveloom" "$BINARY_URL"
else
    echo "Error: curl or wget is required" >&2
    exit 1
fi

# Optional integrity check — skipped silently if no checksum is published yet.
# The published .sha256 references "hiveloom-<platform>" (the release-artifact name),
# but we saved the binary locally as just "hiveloom". Rewrite the filename in the
# checksum file before verifying so `sha256sum -c` finds what's on disk.
if command -v sha256sum &>/dev/null; then
    if curl -fsSL --retry 2 -o "${TMPDIR}/hiveloom.sha256" "${BINARY_URL}.sha256" 2>/dev/null; then
        sed -i "s| hiveloom-${PLATFORM}\$| hiveloom|" "${TMPDIR}/hiveloom.sha256"
        ( cd "$TMPDIR" && sha256sum -c hiveloom.sha256 ) || {
            echo "Checksum verification failed" >&2
            exit 1
        }
    fi
fi

chmod +x "${TMPDIR}/hiveloom"

# ── Install binary ───────────────────────────────────────────────────

echo "Installing to ${INSTALL_DIR}/hiveloom..."
if [ -w "$INSTALL_DIR" ]; then
    mv "${TMPDIR}/hiveloom" "${INSTALL_DIR}/hiveloom"
else
    sudo mv "${TMPDIR}/hiveloom" "${INSTALL_DIR}/hiveloom"
fi

# ── Create data directory ────────────────────────────────────────────

if [ ! -d "$DATA_DIR" ]; then
    echo "Creating data directory ${DATA_DIR}..."
    sudo mkdir -p "$DATA_DIR"
fi

# ── Create systemd service (Linux only) ──────────────────────────────

if [ "$INSTALL_SERVICE" = "1" ] && [ "$(uname -s)" = "Linux" ] && command -v systemctl &>/dev/null; then
    # Create service user if it doesn't exist
    if ! id -u "$SERVICE_USER" &>/dev/null; then
        echo "Creating service user ${SERVICE_USER}..."
        sudo useradd --system --no-create-home --shell /usr/sbin/nologin "$SERVICE_USER" || true
    fi

    sudo chown -R "$SERVICE_USER":"$SERVICE_USER" "$DATA_DIR"

    UNIT_FILE="/etc/systemd/system/hiveloom.service"
    if [ ! -f "$UNIT_FILE" ]; then
        echo "Creating systemd unit ${UNIT_FILE}..."
        sudo tee "$UNIT_FILE" > /dev/null <<UNIT
[Unit]
Description=Hiveloom - Multi-tenant AI Agent Platform
After=network.target

[Service]
Type=simple
User=${SERVICE_USER}
Group=${SERVICE_USER}
ExecStart=${INSTALL_DIR}/hiveloom serve --data-dir ${DATA_DIR}
Restart=on-failure
RestartSec=5
Environment=HIVELOOM_DATA_DIR=${DATA_DIR}
LimitNOFILE=65536

[Install]
WantedBy=multi-user.target
UNIT

        sudo systemctl daemon-reload
        echo "Systemd unit created. Enable with: sudo systemctl enable --now hiveloom"
    else
        echo "Systemd unit already exists; skipping."
    fi
fi

# ── Done ─────────────────────────────────────────────────────────────

echo ""
echo "Hiveloom installed successfully!"
echo ""
echo "  Binary:     ${INSTALL_DIR}/hiveloom"
echo "  Data dir:   ${DATA_DIR}"
echo "  Version:    $(${INSTALL_DIR}/hiveloom version 2>/dev/null || echo "${VERSION}")"
echo ""
echo "Quick start:"
echo "  hiveloom serve                    # start the service"
echo "  hiveloom interactive              # first-run wizard"
echo "  sudo systemctl start hiveloom     # start as system service"
echo ""
