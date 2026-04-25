#!/usr/bin/env bash
# relay-node installer / updater / uninstaller
#
# Usage:
#   bash <(curl -fsSL https://raw.githubusercontent.com/allo-rs/relay-rs/main/scripts/install-node.sh) \
#     --master https://master.example.com:9443 \
#     --ca-b64 <base64 CA bundle from the master> \
#     --enrollment-token <one-time token from `relay-master node-add`> \
#     --node-name <name; must match the one passed to node-add>
#
# Optional environment variables:
#   VERSION        Pin a release tag (default: latest)
#   GITHUB_PROXY   GitHub download proxy prefix

set -euo pipefail

REPO="allo-rs/relay-rs"
INSTALL_BIN="/usr/local/bin/relay-node"
CONFIG_DIR="/etc/relay-rs"
ENV_FILE="$CONFIG_DIR/relay-node.env"
STATE_DIR="/var/lib/relay-node"
SERVICE_FILE="/etc/systemd/system/relay-node.service"
SERVICE_NAME="relay-node"

MASTER=""
CA_B64=""
TOKEN=""
NODE_NAME=""
ACTION=""

[[ $EUID -ne 0 ]] && { echo "Please run as root"; exit 1; }

while [[ $# -gt 0 ]]; do
  case "$1" in
    --master)             MASTER="$2";    shift 2 ;;
    --ca-b64)             CA_B64="$2";    shift 2 ;;
    --enrollment-token)   TOKEN="$2";     shift 2 ;;
    --node-name)          NODE_NAME="$2"; shift 2 ;;
    --uninstall)          ACTION="uninstall"; shift ;;
    --update)             ACTION="update";    shift ;;
    *) echo "Unknown argument: $1"; exit 1 ;;
  esac
done

IS_INSTALLED=false
[[ -f "$INSTALL_BIN" && -f "$STATE_DIR/node_id" ]] && IS_INSTALLED=true

if [[ -z "$ACTION" && -z "$MASTER" ]]; then
  echo ""
  echo "╔══════════════════════════════════════════════════╗"
  echo "║          relay-node 安装管理                      ║"
  echo "╚══════════════════════════════════════════════════╝"
  echo ""
  if $IS_INSTALLED; then
    NID=$(cat "$STATE_DIR/node_id" 2>/dev/null || echo unknown)
    STATUS_LINE=$(systemctl is-active "$SERVICE_NAME" 2>/dev/null || echo "inactive")
    echo "  当前状态: ✅ 已安装  node_id=$NID  systemd=$STATUS_LINE"
    echo ""
    echo "请选择操作:"
    echo "  1. 更新二进制                  (保留证书 + 状态)"
    echo "  2. 重新注册                    (清证书重新走 enrollment)"
    echo "  3. 查看服务状态"
    echo "  4. 卸载"
    echo "  0. 退出"
    read -rp "请选择 [0-4]: " CHOICE
    case "${CHOICE:-0}" in
      1) ACTION="update" ;;
      2) ACTION="reregister" ;;
      3) ACTION="status" ;;
      4) ACTION="uninstall" ;;
      0) exit 0 ;;
      *) echo "无效选项"; exit 1 ;;
    esac
  else
    echo "  当前状态: 未安装"
    echo ""
    echo "首次安装需要以下信息（在 master 上 'relay-master node-add' 获取）："
    echo "  $0 --master <grpc-addr> --ca-b64 <base64> --enrollment-token <t> --node-name <name>"
    exit 1
  fi
fi

if [[ "$ACTION" == "status" ]]; then
  NID=$(cat "$STATE_DIR/node_id" 2>/dev/null || echo unknown)
  echo ""
  echo "node_id:   $NID"
  echo "状态:      $(systemctl is-active "$SERVICE_NAME")"
  echo ""
  systemctl status "$SERVICE_NAME" --no-pager -n 5 2>/dev/null || true
  exit 0
fi

if [[ "$ACTION" == "uninstall" ]]; then
  read -rp "⚠️  Uninstall will remove the cert and state. Continue? [y/N]: " _c
  [[ "${_c,,}" != "y" ]] && exit 0
  systemctl disable --now "$SERVICE_NAME" 2>/dev/null || true
  rm -f "$SERVICE_FILE" "$INSTALL_BIN" "$ENV_FILE"
  rm -rf "$STATE_DIR"
  systemctl daemon-reload
  echo "✅ relay-node uninstalled"
  exit 0
fi

ARCH=$(uname -m)
case "$ARCH" in
  x86_64)        TRIPLE="x86_64-unknown-linux-musl" ;;
  aarch64|arm64) TRIPLE="aarch64-unknown-linux-musl" ;;
  *) echo "Unsupported architecture: $ARCH"; exit 1 ;;
esac

if [[ -z "${VERSION:-}" ]]; then
  VERSION=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" \
    | grep '"tag_name"' | head -1 | cut -d'"' -f4)
  [[ -z "$VERSION" ]] && { echo "Could not resolve latest version"; exit 1; }
fi

_PROXY_DEFAULT="https://gh-proxy.org/"
if [[ -z "${GITHUB_PROXY+x}" ]]; then
  if curl -fsSL --connect-timeout 5 --max-time 8 -o /dev/null "https://github.com" 2>/dev/null; then
    GITHUB_PROXY=""
  else
    GITHUB_PROXY="$_PROXY_DEFAULT"
  fi
fi

download_bin() {
  local artifact="$1" dest="$2"
  local url="${GITHUB_PROXY}https://github.com/$REPO/releases/download/$VERSION/$artifact"
  echo "▶ Downloading $artifact ..."
  local tmp
  tmp=$(mktemp -p /var/tmp 2>/dev/null || mktemp)
  curl -fsSL --connect-timeout 10 --max-time 180 "$url" -o "$tmp" \
    || { rm -f "$tmp"; echo "Download failed: $url"; exit 1; }
  chmod +x "$tmp"; mv "$tmp" "$dest"
}

if [[ "$ACTION" == "update" ]]; then
  download_bin "relay-node-$TRIPLE" "$INSTALL_BIN"
  systemctl restart "$SERVICE_NAME" 2>/dev/null || true
  echo "✅ Update complete (version $VERSION)"
  exit 0
fi

if [[ "$ACTION" == "reregister" ]]; then
  read -rp "master gRPC URL (e.g. https://master:9443): " MASTER
  read -rp "CA bundle (base64): " CA_B64
  read -rp "enrollment token: " TOKEN
  read -rp "node name: " NODE_NAME
  systemctl stop "$SERVICE_NAME" 2>/dev/null || true
  rm -rf "$STATE_DIR"
fi

for v in MASTER CA_B64 TOKEN NODE_NAME; do
  if [[ -z "${!v}" ]]; then
    echo "Error: missing argument --${v,,} (env $v is empty)"
    exit 1
  fi
done

echo "$CA_B64" | base64 -d >/dev/null 2>&1 || { echo "--ca-b64 is not valid base64"; exit 1; }

download_bin "relay-node-$TRIPLE" "$INSTALL_BIN"

mkdir -p "$CONFIG_DIR" "$STATE_DIR"
chmod 700 "$STATE_DIR"

echo "▶ Registering with master..."
if ! MASTER_ADDR="$MASTER" \
     MASTER_CA_PEM_B64="$CA_B64" \
     ENROLLMENT_TOKEN="$TOKEN" \
     NODE_NAME="$NODE_NAME" \
     NODE_STATE_DIR="$STATE_DIR" \
     "$INSTALL_BIN" register; then
  echo "❌ Registration failed. Check master URL, CA bundle, and token."
  exit 1
fi

cat > "$ENV_FILE" <<ENV
MASTER_ADDR=$MASTER
NODE_STATE_DIR=$STATE_DIR
RUST_LOG=info
ENV
chmod 600 "$ENV_FILE"

cat > "$SERVICE_FILE" <<UNIT
[Unit]
Description=relay-node data plane (gRPC over mTLS)
After=network-online.target
Wants=network-online.target

[Service]
EnvironmentFile=$ENV_FILE
ExecStart=$INSTALL_BIN daemon
Restart=on-failure
RestartSec=5
LimitNOFILE=65536

[Install]
WantedBy=multi-user.target
UNIT

systemctl daemon-reload
systemctl enable --now "$SERVICE_NAME"

sleep 2
echo ""
if systemctl is-active --quiet "$SERVICE_NAME"; then
  echo "✅ relay-node installed and running (version $VERSION)"
  echo "   node_id: $(cat "$STATE_DIR/node_id")"
else
  echo "❌ relay-node binary deployed but the service is not active:"
  journalctl -u "$SERVICE_NAME" -n 40 --no-pager
  exit 1
fi
echo ""
echo "Common commands:"
echo "  systemctl status $SERVICE_NAME    service status"
echo "  journalctl -u $SERVICE_NAME -f    follow logs"
echo "  cat $STATE_DIR/node_id            show node_id"
