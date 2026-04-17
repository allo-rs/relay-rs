#!/usr/bin/env bash
set -euo pipefail

# ── 配置 ──────────────────────────────────────────────────────────
REPO="allo-rs/relay-rs"
INSTALL_DIR="/usr/local/bin"
CONFIG_DIR="/etc/relay-rs"
SERVICE_FILE="/etc/systemd/system/relay-rs.service"
BINARY_NAME="relay-rs"

# ── 颜色输出 ──────────────────────────────────────────────────────
RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; NC='\033[0m'
info()  { echo -e "${GREEN}[INFO]${NC}  $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC}  $*"; }
error() { echo -e "${RED}[ERROR]${NC} $*" >&2; exit 1; }

# 国内访问 GitHub 慢时可设置代理，留空则直连
# 例: GITHUB_PROXY="https://gh-proxy.org/"
_PROXY_DEFAULT="https://gh-proxy.org/"
if [[ -z "${GITHUB_PROXY+x}" ]]; then
  if curl -fsSL --connect-timeout 5 --max-time 8 -o /dev/null \
      "https://github.com" 2>/dev/null; then
    GITHUB_PROXY=""
  else
    warn "GitHub 直连超时，自动启用代理: ${_PROXY_DEFAULT}"
    GITHUB_PROXY="$_PROXY_DEFAULT"
  fi
fi

# ── 检查 root ─────────────────────────────────────────────────────
[[ $EUID -ne 0 ]] && error "请以 root 权限运行: bash install.sh"

# ── 检测架构 ──────────────────────────────────────────────────────
ARCH=$(uname -m)
case "$ARCH" in
  x86_64)         ARTIFACT="relay-rs-x86_64" ;;
  aarch64|arm64)  ARTIFACT="relay-rs-aarch64" ;;
  *)              error "不支持的架构: $ARCH" ;;
esac

info "检测到架构: $ARCH，使用产物: $ARTIFACT"

# ── 获取最新版本号 ─────────────────────────────────────────────────
info "获取最新版本..."
LATEST=$(curl -fsSL --connect-timeout 10 --max-time 30 \
  "https://api.github.com/repos/${REPO}/releases/latest" \
  | grep '"tag_name"' | cut -d'"' -f4)
[[ -z "$LATEST" ]] && error "无法获取最新版本，请检查仓库地址或网络"
info "最新版本: $LATEST"

# ── 下载二进制 ────────────────────────────────────────────────────
DOWNLOAD_URL="${GITHUB_PROXY}https://github.com/${REPO}/releases/download/${LATEST}/${ARTIFACT}"
TMP_BIN=$(mktemp)

info "下载 $DOWNLOAD_URL ..."
curl -fsSL --connect-timeout 10 --max-time 120 -o "$TMP_BIN" "$DOWNLOAD_URL" || error "下载失败"
chmod +x "$TMP_BIN"
mv "$TMP_BIN" "${INSTALL_DIR}/${BINARY_NAME}"

info "已安装到 ${INSTALL_DIR}/${BINARY_NAME}"

# ── 初始化配置目录 ────────────────────────────────────────────────
mkdir -p "$CONFIG_DIR"

if [[ ! -f "${CONFIG_DIR}/relay.toml" ]]; then
  cat > "${CONFIG_DIR}/relay.toml" <<'EOF'
# relay-rs 配置文件
# 使用 rr add 添加规则，或直接编辑此文件

# 转发示例（取消注释后生效）：
# [[forward]]
# listen = 10000          # 本机监听端口
# to = "example.com:443" # 目标地址
# proto = "tcp"           # tcp | udp | all

# 防火墙示例：
# [[block]]
# src = "1.2.3.4"         # 封禁来源 IP
EOF
  info "已创建配置文件模板: ${CONFIG_DIR}/relay.toml"
else
  warn "配置文件已存在，跳过创建: ${CONFIG_DIR}/relay.toml"
fi

# ── 安装 systemd 服务 ─────────────────────────────────────────────
cat > "$SERVICE_FILE" <<EOF
[Unit]
Description=relay-rs NAT forwarding daemon
After=network.target

[Service]
ExecStart=${INSTALL_DIR}/${BINARY_NAME} daemon --config ${CONFIG_DIR}/relay.toml
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF

systemctl daemon-reload
systemctl enable relay-rs

# ── 安装 rr 软链 ─────────────────────────────────────────────────
ln -sf "${INSTALL_DIR}/${BINARY_NAME}" "${INSTALL_DIR}/rr"

info "systemd 服务已安装并设置开机自启"
info ""
info "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
info "安装完成！"
info ""
info "使用 rr 命令管理服务："
info "  rr list     查看规则"
info "  rr stats    流量统计"
info "  rr check    检查连通性"
info "  rr ping     探测指定端口（如 rr ping 1.2.3.4:443）"
info "  rr add      添加规则"
info "  rr del      删除规则"
info "  rr edit     编辑规则"
info "  rr mode     切换转发模式（kernel / userspace）"
info "  rr config   编辑配置文件"
info "  rr reload   编辑配置并重启"
info "  rr start    启动"
info "  rr restart  重启"
info "  rr stop     停止"
info "  rr log      查看日志"
info "  rr status   查看状态"
info "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
