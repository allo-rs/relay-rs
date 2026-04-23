#!/usr/bin/env bash
# relay-rs 节点一键安装脚本
# 用法：
#   curl -fsSL https://raw.githubusercontent.com/allo-rs/relay-rs/main/scripts/install-node.sh \
#     | bash -s -- --port 9090 --pubkey-b64 <base64_pubkey>
set -euo pipefail

REPO="allo-rs/relay-rs"
INSTALL_BIN="/usr/local/bin/relay-rs"
CONFIG_DIR="/etc/relay-rs"
CONFIG_FILE="$CONFIG_DIR/relay.toml"
SERVICE_FILE="/etc/systemd/system/relay-rs.service"

PORT=9090
PUBKEY_B64=""

# ── 解析参数 ──────────────────────────────────────────────────────
while [[ $# -gt 0 ]]; do
  case "$1" in
    --port)       PORT="$2";       shift 2 ;;
    --pubkey-b64) PUBKEY_B64="$2"; shift 2 ;;
    *) echo "未知参数: $1"; exit 1 ;;
  esac
done

if [[ -z "$PUBKEY_B64" ]]; then
  echo "错误：缺少 --pubkey-b64 参数"; exit 1
fi

MASTER_PUBKEY=$(echo "$PUBKEY_B64" | base64 -d)

# ── 检测架构 ──────────────────────────────────────────────────────
ARCH=$(uname -m)
case "$ARCH" in
  x86_64)          BIN_ARCH="x86_64-unknown-linux-musl" ;;
  aarch64|arm64)   BIN_ARCH="aarch64-unknown-linux-musl" ;;
  *) echo "不支持的架构: $ARCH"; exit 1 ;;
esac

# ── 下载二进制 ────────────────────────────────────────────────────
echo "▶ 获取最新版本..."
VERSION=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" \
  | grep '"tag_name"' | head -1 | cut -d'"' -f4)

if [[ -z "$VERSION" ]]; then
  echo "错误：无法获取版本号，请检查网络或手动安装"
  exit 1
fi

BIN_URL="https://github.com/$REPO/releases/download/$VERSION/relay-rs-$BIN_ARCH"
echo "▶ 下载 relay-rs $VERSION ($BIN_ARCH)..."
curl -fsSL "$BIN_URL" -o "$INSTALL_BIN"
chmod +x "$INSTALL_BIN"

# ── 写入配置 ──────────────────────────────────────────────────────
echo "▶ 写入配置 $CONFIG_FILE..."
mkdir -p "$CONFIG_DIR"
cat > "$CONFIG_FILE" << TOML
[panel]
mode   = "node"
listen = "0.0.0.0:${PORT}"
master_pubkey = """
${MASTER_PUBKEY}"""
TOML

# ── 创建 systemd 服务 ─────────────────────────────────────────────
echo "▶ 配置 systemd 服务..."
cat > "$SERVICE_FILE" << UNIT
[Unit]
Description=relay-rs node
After=network.target

[Service]
ExecStart=$INSTALL_BIN --config $CONFIG_FILE daemon
Restart=on-failure
RestartSec=5
LimitNOFILE=65536

[Install]
WantedBy=multi-user.target
UNIT

# ── 启动服务 ──────────────────────────────────────────────────────
systemctl daemon-reload
systemctl enable --now relay-rs

echo ""
echo "✓ relay-rs node 安装完成，版本 $VERSION"
echo "  监听端口: $PORT"
echo "  systemctl status relay-rs"
