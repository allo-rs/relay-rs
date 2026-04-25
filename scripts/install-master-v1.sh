#!/usr/bin/env bash
# relay-master v1 安装 / 更新 / 卸载（控制面，纯 gRPC over mTLS）
#
# 与 v0 `install-master.sh` 的差异：
#   · 不再是 `relay-rs daemon` 单进程，v1 控制面走独立二进制 `relay-master`
#   · v1 控制面 = Postgres + gRPC（9443，mTLS）+ v0 HTTP 兼容路由（9090，过渡期保留）
#     → 9443 由 `relay-master daemon` 提供；9090 继续由 `relay-rs daemon` 提供直到 v1.1 移除
#   · 自动生成 CA、dump CA bundle 供 node 侧安装使用
#
# 用法：
#   curl -fsSL https://raw.githubusercontent.com/allo-rs/relay-rs/v1/scripts/install-master-v1.sh | bash
#
# 可选环境变量：
#   VERSION        指定版本号（默认拉 v1 最新）
#   GITHUB_PROXY   GitHub 下载代理前缀（墙内可用 https://gh-proxy.org/）
#   HOSTNAME       gRPC server cert SAN（多个逗号分隔），默认自动探测公网 IP + 127.0.0.1,localhost
#   PANEL_PORT     v1 面板（HTTP）端口，默认 9090（注意：v0 兼容路由暂仍在 v0 daemon 9090；
#                  若仍部署 v0 daemon，请把 v1 panel 改用其它端口，如 9091）
#   GRPC_PORT      v1 gRPC 监听端口，默认 9443

set -euo pipefail

REPO="allo-rs/relay-rs"
INSTALL_BIN_LEGACY="/usr/local/bin/relay-rs"      # v0 主进程
INSTALL_BIN_MASTER="/usr/local/bin/relay-master"  # v1 控制面
CONFIG_DIR="/etc/relay-rs"
ENV_FILE_LEGACY="$CONFIG_DIR/env"                 # v0 面板
ENV_FILE_MASTER="$CONFIG_DIR/master.env"          # v1 控制面
CA_DIR="$CONFIG_DIR/ca"
COMPOSE_DIR="/opt/relay-rs"
COMPOSE_FILE="$COMPOSE_DIR/docker-compose.yml"
SERVICE_FILE_LEGACY="/etc/systemd/system/relay-rs-master.service"
SERVICE_NAME_LEGACY="relay-rs-master"
SERVICE_FILE_MASTER="/etc/systemd/system/relay-master.service"
SERVICE_NAME_MASTER="relay-master"

DB_NAME="relay"
DB_USER="relay"

[[ $EUID -ne 0 ]] && { echo "请以 root 运行"; exit 1; }

# ── 参数解析 ──────────────────────────────────────────────────────
PANEL_PORT="${PANEL_PORT:-9090}"
GRPC_PORT="${GRPC_PORT:-9443}"

# ── 检测是否已装 ──────────────────────────────────────────────────
IS_INSTALLED=false
[[ -f "$INSTALL_BIN_MASTER" && -f "$ENV_FILE_MASTER" ]] && IS_INSTALLED=true

echo ""
echo "╔══════════════════════════════════════════════════╗"
echo "║   relay-master v1 控制面管理脚本                   ║"
echo "╚══════════════════════════════════════════════════╝"
echo ""

if $IS_INSTALLED; then
  CURRENT_VER=$("$INSTALL_BIN_MASTER" --version 2>/dev/null | awk '{print $2}' || echo "未知")
  echo "  当前版本：$CURRENT_VER"
  echo ""
  echo "  1. 更新二进制（保留 CA/数据）"
  echo "  2. 重新生成 server cert（SAN 变更时用，不换 CA）"
  echo "  3. 导出 CA bundle（node 侧安装用）"
  echo "  4. 卸载"
  echo "  5. 退出"
  echo ""
  read -rp "请选择 [1-5]: " CHOICE
  case "${CHOICE:-1}" in
    1) ACTION="update" ;;
    2) ACTION="regen-server-cert" ;;
    3) ACTION="show-ca" ;;
    4) ACTION="uninstall" ;;
    5) exit 0 ;;
    *) echo "无效选择"; exit 1 ;;
  esac
else
  echo "  1. 全新安装"
  echo "  2. 退出"
  echo ""
  read -rp "请选择 [1-2]: " CHOICE
  [[ "${CHOICE:-1}" == "2" ]] && exit 0
  ACTION="install"
fi

# ── show-ca：直接导出 ────────────────────────────────────────────
if [[ "$ACTION" == "show-ca" ]]; then
  RELAY_MASTER_CA_DIR="$CA_DIR" "$INSTALL_BIN_MASTER" ca-show --base64 > /tmp/relay-ca.b64
  echo ""
  echo "✅ CA bundle (base64) 已导出 /tmp/relay-ca.b64"
  echo ""
  echo "node 侧安装命令："
  echo "  bash <(curl -fsSL https://raw.githubusercontent.com/$REPO/v1/scripts/install-node-v1.sh) \\"
  echo "    --master https://<master-host>:$GRPC_PORT \\"
  echo "    --ca-b64 \"\$(cat /tmp/relay-ca.b64)\" \\"
  echo "    --enrollment-token <relay-master node-add --name X 输出的 token>"
  echo ""
  exit 0
fi

# ── regen-server-cert：删 server.pem 让 daemon 下次启动自动重签 ──
if [[ "$ACTION" == "regen-server-cert" ]]; then
  read -rp "server cert SAN 列表（逗号分隔，如 master.example.com,1.2.3.4）: " NEW_SAN
  [[ -z "$NEW_SAN" ]] && { echo "SAN 不能为空"; exit 1; }
  sed -i.bak -E "s|^RELAY_MASTER_HOSTNAME=.*|RELAY_MASTER_HOSTNAME=$NEW_SAN|" "$ENV_FILE_MASTER"
  rm -f "$CA_DIR/server.pem" "$CA_DIR/server.key"
  systemctl restart "$SERVICE_NAME_MASTER"
  sleep 2
  echo "✅ server cert 已重新签发（SAN: $NEW_SAN）"
  exit 0
fi

# ── 卸载 ─────────────────────────────────────────────────────────
if [[ "$ACTION" == "uninstall" ]]; then
  read -rp "⚠️  卸载会删除 CA、enrollment tokens 和 v1 数据。确认？[y/N]: " _confirm
  [[ "${_confirm,,}" != "y" ]] && { echo "已取消"; exit 0; }
  systemctl disable --now "$SERVICE_NAME_MASTER" 2>/dev/null || true
  rm -f "$SERVICE_FILE_MASTER" "$INSTALL_BIN_MASTER" "$ENV_FILE_MASTER"
  rm -rf "$CA_DIR"
  systemctl daemon-reload
  echo "✅ relay-master v1 已卸载（v0 relay-rs daemon/DB 未动）"
  exit 0
fi

# ── 版本 / 架构 / 代理 ───────────────────────────────────────────
ARCH=$(uname -m)
case "$ARCH" in
  x86_64)        TRIPLE="x86_64-unknown-linux-musl" ;;
  aarch64|arm64) TRIPLE="aarch64-unknown-linux-musl" ;;
  *) echo "不支持的架构: $ARCH"; exit 1 ;;
esac

if [[ -z "${VERSION:-}" ]]; then
  echo "▶ 获取最新 v1 版本..."
  # v1 发布 tag 约定以 v1.* 开头；这里取 latest release 即可（假设维护者发 v1.x 时就是 latest）
  VERSION=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" \
    | grep '"tag_name"' | head -1 | cut -d'"' -f4)
  [[ -z "$VERSION" ]] && { echo "无法获取版本号"; exit 1; }
fi

_PROXY_DEFAULT="https://gh-proxy.org/"
if [[ -z "${GITHUB_PROXY+x}" ]]; then
  if curl -fsSL --connect-timeout 5 --max-time 8 -o /dev/null "https://github.com" 2>/dev/null; then
    GITHUB_PROXY=""
  else
    echo "GitHub 直连慢，启用代理 $_PROXY_DEFAULT"
    GITHUB_PROXY="$_PROXY_DEFAULT"
  fi
fi

download_bin() {
  local artifact="$1" dest="$2"
  local url="${GITHUB_PROXY}https://github.com/$REPO/releases/download/$VERSION/$artifact"
  echo "▶ 下载 $artifact ..."
  local tmp; tmp=$(mktemp)
  curl -fsSL --connect-timeout 10 --max-time 180 "$url" -o "$tmp" \
    || { rm -f "$tmp"; echo "下载失败: $url"; exit 1; }
  chmod +x "$tmp"; mv "$tmp" "$dest"
}

# ── update 分支 ──────────────────────────────────────────────────
if [[ "$ACTION" == "update" ]]; then
  download_bin "relay-master-$TRIPLE" "$INSTALL_BIN_MASTER"
  systemctl restart "$SERVICE_NAME_MASTER"
  echo "✅ 更新完成（版本 $VERSION）"
  exit 0
fi

# ── install 分支 ─────────────────────────────────────────────────

# 依赖：要求 v0 已装好（Postgres、relay-rs daemon），v1 控制面复用其 DB
if [[ ! -f "$COMPOSE_FILE" ]] || [[ ! -f "$ENV_FILE_LEGACY" ]]; then
  echo "⚠️  未检测到 v0 安装（$ENV_FILE_LEGACY / $COMPOSE_FILE 缺失）。"
  echo "v1 控制面目前仍依赖 v0 的 Postgres 与 HTTP 面板（过渡期）。"
  read -rp "是否先运行 v0 install-master.sh？[Y/n]: " _ans
  if [[ "${_ans:-Y}" =~ ^[Yy]$ ]]; then
    bash <(curl -fsSL "https://raw.githubusercontent.com/$REPO/main/scripts/install-master.sh") || exit 1
  else
    echo "请先部署 v0 后再安装 v1 控制面。"; exit 1
  fi
fi

# 自动探测 SAN
if [[ -z "${HOSTNAME:-}" ]]; then
  PUB_IP=$(curl -fsSL --connect-timeout 5 https://api.ipify.org 2>/dev/null || true)
  HOSTNAME="127.0.0.1,localhost${PUB_IP:+,$PUB_IP}"
fi
echo "▶ server cert SAN: $HOSTNAME"

download_bin "relay-master-$TRIPLE" "$INSTALL_BIN_MASTER"

# ── env 文件 ─────────────────────────────────────────────────────
mkdir -p "$CONFIG_DIR" "$CA_DIR"

# 从 v0 env 文件读 DATABASE_URL（v1 master 需要它做 schema 迁移 + 运行时读段落）
V0_DB_URL=""
if [[ -f "$ENV_FILE_LEGACY" ]]; then
  V0_DB_URL=$(grep -E '^DATABASE_URL=' "$ENV_FILE_LEGACY" | head -n1 | cut -d= -f2-)
fi
if [[ -z "$V0_DB_URL" ]]; then
  echo "⚠️  从 $ENV_FILE_LEGACY 未能读到 DATABASE_URL；v1 master 启动需要它。"
  read -rp "请输入 DATABASE_URL (形如 postgresql://user:pass@127.0.0.1:5432/relay): " V0_DB_URL
  [[ -z "$V0_DB_URL" ]] && { echo "DATABASE_URL 必填"; exit 1; }
fi

cat > "$ENV_FILE_MASTER" <<ENV
# relay-master v1 控制面配置
RELAY_MASTER_CA_DIR=$CA_DIR
RELAY_MASTER_TOKEN_DIR=$CA_DIR/enrollment-tokens
RELAY_MASTER_LISTEN=0.0.0.0:$GRPC_PORT
RELAY_MASTER_HOSTNAME=$HOSTNAME
DATABASE_URL=$V0_DB_URL
# v1 Web 面板（HTTP；建议外置 nginx/cloudflare 终结 TLS）
RELAY_PANEL_LISTEN=0.0.0.0:$PANEL_PORT
RELAY_PANEL_EXTERNAL_URL=https://your-panel.example/
# 32B 随机 hex；切勿轻易轮换（轮换会让所有用户立刻退登）
RELAY_PANEL_JWT_SECRET=$(openssl rand -hex 32)
RUST_LOG=info
ENV
chmod 600 "$ENV_FILE_MASTER"

# ── systemd unit ─────────────────────────────────────────────────
cat > "$SERVICE_FILE_MASTER" <<UNIT
[Unit]
Description=relay-master v1 control plane (gRPC over mTLS)
After=network.target docker.service
Requires=docker.service

[Service]
EnvironmentFile=$ENV_FILE_MASTER
ExecStart=$INSTALL_BIN_MASTER daemon
Restart=on-failure
RestartSec=5
LimitNOFILE=65536

[Install]
WantedBy=multi-user.target
UNIT

systemctl daemon-reload
systemctl enable --now "$SERVICE_NAME_MASTER"
sleep 2

if ! systemctl is-active --quiet "$SERVICE_NAME_MASTER"; then
  echo "❌ relay-master 启动失败："
  journalctl -u "$SERVICE_NAME_MASTER" -n 40 --no-pager
  exit 1
fi

# ── 导出 CA bundle 给 node 用 ────────────────────────────────────
CA_B64=$(RELAY_MASTER_CA_DIR="$CA_DIR" "$INSTALL_BIN_MASTER" ca-show --base64)
echo "$CA_B64" > /tmp/relay-ca.b64
chmod 600 /tmp/relay-ca.b64

echo ""
echo "✅ relay-master v1 安装完成（版本 $VERSION）"
echo ""
echo "监听："
echo "  · v0 HTTP 面板：http://<host>:9090         （兼容期，v1.1 移除）"
echo "  · v1 Web 面板 ：http://<host>:$PANEL_PORT  （建议外置 TLS）"
echo "  · v1 gRPC mTLS：https://<host>:$GRPC_PORT  （node 接入）"
echo ""
echo "⚠️  请编辑 $ENV_FILE_MASTER 把 RELAY_PANEL_EXTERNAL_URL 改成真实公网入口（含协议）。"
echo ""
echo "添加 node："
echo "  1) 在本机生成一次性 enrollment token："
echo "       $INSTALL_BIN_MASTER node-add --name <node-name>"
echo "     输出形如：enrollment token: AbC...xyz"
echo ""
echo "  2) 在 node 机器上执行："
echo "       bash <(curl -fsSL https://raw.githubusercontent.com/$REPO/v1/scripts/install-node-v1.sh) \\"
echo "         --master https://<master-host>:$GRPC_PORT \\"
echo "         --ca-b64 \"\$(cat /tmp/relay-ca.b64 | ssh root@master cat)\" \\"
echo "         --enrollment-token <上一步的 token> \\"
echo "         --node-name <node-name>"
echo ""
echo "CA bundle (base64) 已缓存到 /tmp/relay-ca.b64"
echo ""
echo "常用命令："
echo "  systemctl status $SERVICE_NAME_MASTER          查看服务状态"
echo "  $INSTALL_BIN_MASTER node-add --name X          生成 enrollment token"
echo "  $INSTALL_BIN_MASTER ca-show --base64           再次导出 CA bundle"
echo "  journalctl -u $SERVICE_NAME_MASTER -f          实时日志"
