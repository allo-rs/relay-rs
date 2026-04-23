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

[[ $EUID -ne 0 ]] && { echo "请以 root 运行"; exit 1; }

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

# ── 选择下载代理 ──────────────────────────────────────────────────
_PROXY_DEFAULT="https://gh-proxy.org/"
if [[ -z "${GITHUB_PROXY+x}" ]]; then
  if curl -fsSL --connect-timeout 5 --max-time 8 -o /dev/null "https://github.com" 2>/dev/null; then
    GITHUB_PROXY=""
  else
    echo "GitHub 直连慢，启用代理 $_PROXY_DEFAULT"
    GITHUB_PROXY="$_PROXY_DEFAULT"
  fi
fi

# ── 下载二进制 ────────────────────────────────────────────────────
echo "▶ 获取最新版本..."
VERSION=$(curl -fsSL --connect-timeout 10 --max-time 30 \
  "https://api.github.com/repos/$REPO/releases/latest" \
  | grep '"tag_name"' | head -1 | cut -d'"' -f4)

if [[ -z "$VERSION" ]]; then
  echo "错误：无法获取版本号，请检查网络或手动安装"
  exit 1
fi

BIN_URL="${GITHUB_PROXY}https://github.com/$REPO/releases/download/$VERSION/relay-rs-$BIN_ARCH"
echo "▶ 下载 relay-rs $VERSION ($BIN_ARCH)..."
TMP_BIN=$(mktemp)
curl -fsSL --connect-timeout 10 --max-time 120 "$BIN_URL" -o "$TMP_BIN" || { rm -f "$TMP_BIN"; exit 1; }
chmod +x "$TMP_BIN"
mv "$TMP_BIN" "$INSTALL_BIN"

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
