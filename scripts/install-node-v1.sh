#!/usr/bin/env bash
# relay-node v1 安装 / 更新 / 卸载（数据面，mTLS 接入 v1 master）
#
# v0 与 v1 共存期说明：
#   · v0 node（relay-rs-node.service，HTTP+JWT）可与 v1 node（relay-node.service，gRPC+mTLS）并存
#   · v1.1 起将下线 v0 路径
#
# 用法：
#   bash <(curl -fsSL https://raw.githubusercontent.com/allo-rs/relay-rs/v1/scripts/install-node-v1.sh) \
#     --master https://master.example.com:9443 \
#     --ca-b64 <master 侧 /tmp/relay-ca.b64 的内容> \
#     --enrollment-token <relay-master node-add 的输出> \
#     --node-name <与 node-add --name 一致>
#
# 可选环境变量：
#   VERSION        指定版本号（默认拉 latest）
#   GITHUB_PROXY   GitHub 下载代理前缀

set -euo pipefail

REPO="allo-rs/relay-rs"
INSTALL_BIN="/usr/local/bin/relay-node"
CONFIG_DIR="/etc/relay-rs"
ENV_FILE="$CONFIG_DIR/node-v1.env"
STATE_DIR="/var/lib/relay-node"
SERVICE_FILE="/etc/systemd/system/relay-node.service"
SERVICE_NAME="relay-node"

MASTER=""
CA_B64=""
TOKEN=""
NODE_NAME=""
ACTION=""

[[ $EUID -ne 0 ]] && { echo "请以 root 运行"; exit 1; }

# ── 参数解析 ──────────────────────────────────────────────────────
while [[ $# -gt 0 ]]; do
  case "$1" in
    --master)             MASTER="$2";    shift 2 ;;
    --ca-b64)             CA_B64="$2";    shift 2 ;;
    --enrollment-token)   TOKEN="$2";     shift 2 ;;
    --node-name)          NODE_NAME="$2"; shift 2 ;;
    --uninstall)          ACTION="uninstall"; shift ;;
    --update)             ACTION="update";    shift ;;
    *) echo "未知参数: $1"; exit 1 ;;
  esac
done

IS_INSTALLED=false
[[ -f "$INSTALL_BIN" && -f "$STATE_DIR/node_id" ]] && IS_INSTALLED=true

# 无参数 → 进交互菜单
if [[ -z "$ACTION" && -z "$MASTER" ]]; then
  echo ""
  echo "╔══════════════════════════════════════════════════╗"
  echo "║       relay-node v1 数据面管理脚本                 ║"
  echo "╚══════════════════════════════════════════════════╝"
  echo ""
  if $IS_INSTALLED; then
    echo "  当前 node_id：$(cat $STATE_DIR/node_id 2>/dev/null || echo 未知)"
    echo ""
    echo "  1. 更新二进制（保留证书和配置）"
    echo "  2. 重新注册（清除证书，重新走 enrollment token）"
    echo "  3. 卸载"
    echo "  4. 退出"
    read -rp "请选择 [1-4]: " CHOICE
    case "${CHOICE:-1}" in
      1) ACTION="update" ;;
      2) ACTION="reregister" ;;
      3) ACTION="uninstall" ;;
      4) exit 0 ;;
      *) echo "无效选择"; exit 1 ;;
    esac
  else
    echo "未检测到已安装的 relay-node。请按参数方式运行："
    echo "  $0 --master <grpc-addr> --ca-b64 <base64> --enrollment-token <t> --node-name <name>"
    exit 1
  fi
fi

# ── 卸载 ─────────────────────────────────────────────────────────
if [[ "$ACTION" == "uninstall" ]]; then
  read -rp "⚠️  卸载将删除证书和状态，确认？[y/N]: " _c
  [[ "${_c,,}" != "y" ]] && exit 0
  systemctl disable --now "$SERVICE_NAME" 2>/dev/null || true
  rm -f "$SERVICE_FILE" "$INSTALL_BIN" "$ENV_FILE"
  rm -rf "$STATE_DIR"
  systemctl daemon-reload
  echo "✅ relay-node v1 已卸载"
  exit 0
fi

# ── 架构/版本/代理 ───────────────────────────────────────────────
ARCH=$(uname -m)
case "$ARCH" in
  x86_64)        TRIPLE="x86_64-unknown-linux-musl" ;;
  aarch64|arm64) TRIPLE="aarch64-unknown-linux-musl" ;;
  *) echo "不支持的架构: $ARCH"; exit 1 ;;
esac

if [[ -z "${VERSION:-}" ]]; then
  VERSION=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" \
    | grep '"tag_name"' | head -1 | cut -d'"' -f4)
  [[ -z "$VERSION" ]] && { echo "无法获取版本号"; exit 1; }
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
  echo "▶ 下载 $artifact ..."
  local tmp; tmp=$(mktemp)
  curl -fsSL --connect-timeout 10 --max-time 180 "$url" -o "$tmp" \
    || { rm -f "$tmp"; echo "下载失败: $url"; exit 1; }
  chmod +x "$tmp"; mv "$tmp" "$dest"
}

# ── update ───────────────────────────────────────────────────────
if [[ "$ACTION" == "update" ]]; then
  download_bin "relay-node-$TRIPLE" "$INSTALL_BIN"
  systemctl restart "$SERVICE_NAME" 2>/dev/null || true
  echo "✅ 更新完成（版本 $VERSION）"
  exit 0
fi

# ── 全新安装 / 重新注册 ─────────────────────────────────────────
if [[ "$ACTION" == "reregister" ]]; then
  # 重新收集注册参数
  read -rp "master gRPC 地址（如 https://master:9443）: " MASTER
  read -rp "CA bundle (base64): " CA_B64
  read -rp "enrollment token: " TOKEN
  read -rp "node name: " NODE_NAME
  systemctl stop "$SERVICE_NAME" 2>/dev/null || true
  rm -rf "$STATE_DIR"
fi

for v in MASTER CA_B64 TOKEN NODE_NAME; do
  if [[ -z "${!v}" ]]; then echo "错误：缺少参数 --${v,,} / $v 为空"; exit 1; fi
done

# 验证 base64
echo "$CA_B64" | base64 -d >/dev/null 2>&1 || { echo "--ca-b64 不是合法 base64"; exit 1; }

download_bin "relay-node-$TRIPLE" "$INSTALL_BIN"

mkdir -p "$CONFIG_DIR" "$STATE_DIR"
chmod 700 "$STATE_DIR"

# ── 首次注册（阻塞式，失败则直接退出）─────────────────────────
echo "▶ 向 master 注册..."
if ! MASTER_ADDR="$MASTER" \
     MASTER_CA_PEM_B64="$CA_B64" \
     ENROLLMENT_TOKEN="$TOKEN" \
     NODE_NAME="$NODE_NAME" \
     NODE_STATE_DIR="$STATE_DIR" \
     "$INSTALL_BIN" register; then
  echo "❌ 注册失败。请检查 master 地址、CA、token 是否正确。"
  exit 1
fi

# ── env 文件（供 daemon 子命令使用）─────────────────────────────
cat > "$ENV_FILE" <<ENV
MASTER_ADDR=$MASTER
NODE_STATE_DIR=$STATE_DIR
RUST_LOG=info
ENV
chmod 600 "$ENV_FILE"

# ── systemd unit ─────────────────────────────────────────────────
cat > "$SERVICE_FILE" <<UNIT
[Unit]
Description=relay-node v1 data plane (gRPC over mTLS)
After=network.target

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
  echo "✅ relay-node v1 安装并启动完成（版本 $VERSION）"
  echo "   node_id: $(cat $STATE_DIR/node_id)"
else
  echo "⚠️  relay-node 二进制已部署，但 daemon 尚未完全实现（需 M3 Sync 落地）。"
  echo "   证书已签发，可在 M3 发布后升级即可开始工作。"
fi
echo ""
echo "常用命令："
echo "  systemctl status $SERVICE_NAME    查看服务状态"
echo "  journalctl -u $SERVICE_NAME -f    实时日志"
echo "  cat $STATE_DIR/node_id            查看 node_id"
