#!/usr/bin/env bash
# relay-rs 主控（master）安装 / 更新 / 卸载脚本
#
# 用法：
#   curl -fsSL https://raw.githubusercontent.com/allo-rs/relay-rs/main/scripts/install-master.sh | bash
#
# 可选环境变量：
#   VERSION        指定版本号（默认拉 latest）
#   GITHUB_PROXY   GitHub 下载代理前缀（墙内可用 https://gh-proxy.org/）
set -euo pipefail

REPO="allo-rs/relay-rs"
INSTALL_BIN="/usr/local/bin/relay-rs"
CONFIG_DIR="/etc/relay-rs"
ENV_FILE="$CONFIG_DIR/env"
COMPOSE_DIR="/opt/relay-rs"
COMPOSE_FILE="$COMPOSE_DIR/docker-compose.yml"
SERVICE_FILE="/etc/systemd/system/relay-rs-master.service"
SERVICE_NAME="relay-rs-master"

DB_NAME="relay"
DB_USER="relay"

# ── 权限检查 ──────────────────────────────────────────────────────
[[ $EUID -ne 0 ]] && { echo "请以 root 运行"; exit 1; }

# ── 检测是否已安装 ────────────────────────────────────────────────
IS_INSTALLED=false
[[ -f "$INSTALL_BIN" && -f "$ENV_FILE" ]] && IS_INSTALLED=true

# ── 菜单 ─────────────────────────────────────────────────────────
echo ""
echo "╔══════════════════════════════════════════════════╗"
echo "║            relay-rs 主控管理脚本                   ║"
echo "╚══════════════════════════════════════════════════╝"
echo ""

if $IS_INSTALLED; then
  CURRENT_VER=$("$INSTALL_BIN" --version 2>/dev/null | awk '{print $2}' || echo "未知")
  echo "  当前版本：$CURRENT_VER"
  echo ""
  echo "  1. 更新二进制（保留数据和配置）"
  echo "  2. 全新安装（清除现有数据）"
  echo "  3. 卸载"
  echo "  4. 退出"
  echo ""
  read -rp "请选择 [1-4]: " CHOICE
  CHOICE="${CHOICE:-1}"
else
  echo "  1. 全新安装"
  echo "  2. 退出"
  echo ""
  read -rp "请选择 [1-2]: " CHOICE
  CHOICE="${CHOICE:-1}"
  # 映射到统一动作
  [[ "$CHOICE" == "2" ]] && exit 0
  CHOICE="install"
fi

case "$CHOICE" in
  1) $IS_INSTALLED && ACTION="update"   || ACTION="install" ;;
  2) $IS_INSTALLED && ACTION="install"  || exit 0 ;;
  3) $IS_INSTALLED && ACTION="uninstall"|| exit 0 ;;
  4) exit 0 ;;
  *) echo "无效选择"; exit 1 ;;
esac

# ── 卸载 ─────────────────────────────────────────────────────────
if [[ "$ACTION" == "uninstall" ]]; then
  read -rp "确认卸载？这将停止服务并删除所有数据 [y/N]: " _confirm
  [[ "${_confirm,,}" != "y" ]] && { echo "已取消"; exit 0; }

  systemctl disable --now "$SERVICE_NAME" 2>/dev/null || true
  rm -f "$SERVICE_FILE"
  systemctl daemon-reload

  if [[ -f "$COMPOSE_FILE" ]]; then
    docker compose -f "$COMPOSE_FILE" down -v 2>/dev/null || true
  fi

  rm -rf "$CONFIG_DIR" "$COMPOSE_DIR"
  rm -f "$INSTALL_BIN" /usr/local/bin/rr

  echo "✅ 卸载完成"
  exit 0
fi

# ── 全新安装前确认（已有数据时警告）────────────────────────────
if [[ "$ACTION" == "install" ]] && $IS_INSTALLED; then
  read -rp "⚠️  全新安装将清除现有数据库和配置，确认继续？[y/N]: " _confirm
  [[ "${_confirm,,}" != "y" ]] && { echo "已取消"; exit 0; }

  systemctl stop "$SERVICE_NAME" 2>/dev/null || true
  if [[ -f "$COMPOSE_FILE" ]]; then
    docker compose -f "$COMPOSE_FILE" down -v 2>/dev/null || true
  fi
  rm -f "$ENV_FILE" "$COMPOSE_FILE"
fi

# ── 询问面板端口（全新安装时）─────────────────────────────────
PANEL_PORT=9090
if [[ "$ACTION" == "install" ]]; then
  read -rp "面板监听端口 [9090]: " _port
  PANEL_PORT="${_port:-9090}"
fi

# ── 检测架构 ──────────────────────────────────────────────────────
ARCH=$(uname -m)
case "$ARCH" in
  x86_64)        ARTIFACT="relay-rs-x86_64-unknown-linux-musl" ;;
  aarch64|arm64) ARTIFACT="relay-rs-aarch64-unknown-linux-musl" ;;
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

# ── 下载二进制 ────────────────────────────────────────────────────
BIN_URL="${GITHUB_PROXY}https://github.com/$REPO/releases/download/$VERSION/$ARTIFACT"
echo "▶ 下载 relay-rs $VERSION..."
TMP_BIN=$(mktemp)
curl -fsSL --connect-timeout 10 --max-time 120 "$BIN_URL" -o "$TMP_BIN" || { rm -f "$TMP_BIN"; exit 1; }
chmod +x "$TMP_BIN"
mv "$TMP_BIN" "$INSTALL_BIN"
ln -sf "$INSTALL_BIN" /usr/local/bin/rr

# ── 仅更新时：重启服务后退出 ──────────────────────────────────
if [[ "$ACTION" == "update" ]]; then
  systemctl restart "$SERVICE_NAME"
  echo ""
  echo "✅ 更新完成（版本 $VERSION）"
  echo "   systemctl status $SERVICE_NAME"
  exit 0
fi

# ── 以下为全新安装流程 ────────────────────────────────────────────

# ── 安装 Docker（如未安装）────────────────────────────────────────
if ! command -v docker &>/dev/null; then
  echo "▶ 安装 Docker..."
  curl -fsSL https://get.docker.com | sh
  systemctl enable --now docker
fi

# ── 生成随机 DB 密码 / 构造连接串 ────────────────────────────────
DB_PASS=$(head -c 24 /dev/urandom | base64 | tr -d '=+/' | cut -c1-24)
DB_URL="postgresql://${DB_USER}:${DB_PASS}@127.0.0.1:5432/${DB_NAME}?sslmode=disable"

# ── 写 docker-compose.yml ─────────────────────────────────────────
mkdir -p "$COMPOSE_DIR"
cat > "$COMPOSE_FILE" <<YAML
services:
  postgres:
    image: postgres:16-alpine
    restart: unless-stopped
    environment:
      POSTGRES_DB: ${DB_NAME}
      POSTGRES_USER: ${DB_USER}
      POSTGRES_PASSWORD: ${DB_PASS}
    volumes:
      - pgdata:/var/lib/postgresql/data
    ports:
      - "127.0.0.1:5432:5432"
volumes:
  pgdata:
YAML

# ── 启动 PostgreSQL ───────────────────────────────────────────────
echo "▶ 启动 PostgreSQL..."
docker compose -f "$COMPOSE_FILE" up -d

echo "▶ 等待 PostgreSQL 就绪..."
for i in $(seq 1 30); do
  if docker compose -f "$COMPOSE_FILE" exec -T postgres \
      pg_isready -U "$DB_USER" -d "$DB_NAME" &>/dev/null; then
    break
  fi
  sleep 1
  if [[ $i -eq 30 ]]; then
    echo "PostgreSQL 启动超时，请检查：docker compose -f $COMPOSE_FILE logs"
    exit 1
  fi
done
echo "▶ PostgreSQL 就绪"

# ── 写 env 文件 ───────────────────────────────────────────────────
mkdir -p "$CONFIG_DIR"
cat > "$ENV_FILE" <<ENV
DATABASE_URL=${DB_URL}
PANEL_LISTEN=0.0.0.0:${PANEL_PORT}
ENV
chmod 600 "$ENV_FILE"
echo "▶ 已写入 $ENV_FILE"

# ── systemd 服务 ──────────────────────────────────────────────────
cat > "$SERVICE_FILE" <<UNIT
[Unit]
Description=relay-rs master panel
After=network.target docker.service
Requires=docker.service

[Service]
EnvironmentFile=/etc/relay-rs/env
ExecStart=$INSTALL_BIN daemon
Restart=on-failure
RestartSec=5
LimitNOFILE=65536

[Install]
WantedBy=multi-user.target
UNIT

systemctl daemon-reload
systemctl enable --now "$SERVICE_NAME"

echo ""
echo "✅ 主控安装完成（版本 $VERSION）"
echo ""
echo "访问面板：http://<server-ip>:${PANEL_PORT}"
echo "   · 首次访问为开放模式，可直接进入"
echo "   · 在「设置 → Discourse 接入」填入 URL/secret 启用登录"
echo ""
echo "节点接入（在节点面板添加后会得到公钥）："
echo "  bash <(curl -fsSL https://raw.githubusercontent.com/allo-rs/relay-rs/main/scripts/install-node.sh) \\"
echo "    --port 19090 --pubkey-b64 <主控公钥的 base64>"
echo ""
echo "常用命令："
echo "  systemctl status $SERVICE_NAME    查看服务状态"
echo "  rr list                           查看转发规则"
echo "  rr add                            添加转发规则"
echo "  rr panel-reset-auth               清除 Discourse 配置（锁死救援）"
echo "  journalctl -u $SERVICE_NAME -f    实时日志"
echo ""
echo "数据库（PostgreSQL）："
echo "  docker compose -f $COMPOSE_FILE ps"
echo "  docker compose -f $COMPOSE_FILE logs postgres"
