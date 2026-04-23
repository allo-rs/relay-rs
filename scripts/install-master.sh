#!/usr/bin/env bash
# relay-rs 主控（master）一键安装脚本
#
# 用法：
#   curl -fsSL https://raw.githubusercontent.com/allo-rs/relay-rs/main/scripts/install-master.sh \
#     | bash -s -- --db "postgresql://user:pass@host:5432/relay?sslmode=disable" [--port 9090]
#
# 可选环境变量：
#   VERSION        指定版本号（默认拉 latest）
#   GITHUB_PROXY   GitHub 下载代理前缀（墙内可用 https://gh-proxy.org/）
set -euo pipefail

REPO="allo-rs/relay-rs"
INSTALL_BIN="/usr/local/bin/relay-rs"
CONFIG_DIR="/etc/relay-rs"
CONFIG_FILE="$CONFIG_DIR/relay.toml"
SERVICE_FILE="/etc/systemd/system/relay-rs.service"

PORT=9090
DB_URL=""

# ── 解析参数 ──────────────────────────────────────────────────────
while [[ $# -gt 0 ]]; do
  case "$1" in
    --port) PORT="$2"; shift 2 ;;
    --db)   DB_URL="$2"; shift 2 ;;
    *) echo "未知参数: $1"; exit 1 ;;
  esac
done

[[ $EUID -ne 0 ]] && { echo "请以 root 运行"; exit 1; }

if [[ -z "$DB_URL" ]]; then
  echo "错误：缺少 --db 参数（PostgreSQL 连接串）"
  echo "示例：--db \"postgresql://relay:PASS@127.0.0.1:5432/relay?sslmode=disable\""
  exit 1
fi

# ── 检测架构 ──────────────────────────────────────────────────────
ARCH=$(uname -m)
case "$ARCH" in
  x86_64)         ARTIFACT="relay-rs-x86_64-unknown-linux-musl" ;;
  aarch64|arm64)  ARTIFACT="relay-rs-aarch64-unknown-linux-musl" ;;
  *) echo "不支持的架构: $ARCH"; exit 1 ;;
esac

# ── 选择版本 ──────────────────────────────────────────────────────
if [[ -z "${VERSION:-}" ]]; then
  echo "▶ 获取最新版本..."
  VERSION=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" \
    | grep '"tag_name"' | head -1 | cut -d'"' -f4)
  [[ -z "$VERSION" ]] && { echo "无法获取版本号"; exit 1; }
fi

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

BIN_URL="${GITHUB_PROXY}https://github.com/$REPO/releases/download/$VERSION/$ARTIFACT"
echo "▶ 下载 $BIN_URL"
curl -fsSL --connect-timeout 10 --max-time 120 "$BIN_URL" -o "$INSTALL_BIN"
chmod +x "$INSTALL_BIN"
ln -sf "$INSTALL_BIN" /usr/local/bin/rr

# ── 生成 secret 与配置文件 ────────────────────────────────────────
mkdir -p "$CONFIG_DIR"
if [[ -f "$CONFIG_FILE" ]]; then
  echo "⚠️  配置文件已存在：$CONFIG_FILE（跳过写入，仅更新二进制）"
else
  SECRET=$(head -c 32 /dev/urandom | base64 | tr -d '=+/' | cut -c1-40)
  cat > "$CONFIG_FILE" <<TOML
mode = "relay"
forward = []
block = []

[panel]
mode = "master"
listen = "0.0.0.0:${PORT}"
secret = "${SECRET}"
database_url = "${DB_URL}"
TOML
  echo "▶ 已写入配置：$CONFIG_FILE"
fi

# ── 生成 Ed25519 主控密钥（首次）──────────────────────────────────
echo "▶ 初始化主控 Ed25519 密钥..."
"$INSTALL_BIN" --config "$CONFIG_FILE" panel-init

# ── systemd 服务 ──────────────────────────────────────────────────
cat > "$SERVICE_FILE" <<UNIT
[Unit]
Description=relay-rs master panel
After=network.target

[Service]
ExecStart=$INSTALL_BIN --config $CONFIG_FILE daemon
Restart=on-failure
RestartSec=5
LimitNOFILE=65536

[Install]
WantedBy=multi-user.target
UNIT

systemctl daemon-reload
systemctl enable --now relay-rs

echo ""
echo "✅ 主控安装完成（版本 $VERSION）"
echo ""
echo "访问面板：http://<server-ip>:${PORT}"
echo "   · 首次访问为开放模式，可直接进入"
echo "   · 在「设置 → Discourse 接入」填入 URL/secret 启用登录"
echo ""
echo "节点接入（在节点面板添加后会得到公钥）："
echo "  bash <(curl -fsSL https://raw.githubusercontent.com/allo-rs/relay-rs/main/scripts/install-node.sh) \\"
echo "    --port 9090 --pubkey-b64 <主控公钥的 base64>"
echo ""
echo "常用命令："
echo "  systemctl status relay-rs         查看服务状态"
echo "  rr panel-reset-auth               清除 Discourse 配置（锁死救援）"
echo "  journalctl -u relay-rs -f         实时日志"
