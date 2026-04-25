#!/usr/bin/env bash
# relay-master installer / updater / uninstaller
#
# Usage:
#   curl -fsSL "https://raw.githubusercontent.com/allo-rs/relay-rs/main/scripts/install-master.sh?$(date +%s)" | bash
#
# Optional environment variables:
#   VERSION        Pin a release tag (default: latest)
#   GITHUB_PROXY   GitHub download proxy prefix (e.g. https://gh-proxy.org/)
#   HOSTNAME       Comma-separated SAN list for the gRPC server cert.
#                  Defaults to 127.0.0.1,localhost plus auto-detected public IP.
#   PANEL_PORT     Web panel HTTP port (default 9090)
#   GRPC_PORT      gRPC mTLS port (default 9443)
#   DATABASE_URL   If set and reachable via psql, reused as-is.
#                  Otherwise the script will spin up a Postgres container or
#                  prompt for a URL when Docker is unavailable.

set -euo pipefail

REPO="allo-rs/relay-rs"
INSTALL_BIN="/usr/local/bin/relay-master"
CONFIG_DIR="/etc/relay-rs"
ENV_FILE="$CONFIG_DIR/relay-master.env"
CA_DIR="$CONFIG_DIR/ca"
SERVICE_FILE="/etc/systemd/system/relay-master.service"
SERVICE_NAME="relay-master"

PG_CONTAINER="relay-postgres"
PG_DB="relay"
PG_USER="relay"

[[ $EUID -ne 0 ]] && { echo "Please run as root"; exit 1; }

PANEL_PORT="${PANEL_PORT:-9090}"
GRPC_PORT="${GRPC_PORT:-9443}"

IS_INSTALLED=false
[[ -f "$INSTALL_BIN" && -f "$ENV_FILE" ]] && IS_INSTALLED=true

echo ""
echo "╔══════════════════════════════════════════════════╗"
echo "║          relay-master 安装管理                    ║"
echo "╚══════════════════════════════════════════════════╝"
echo ""

if $IS_INSTALLED; then
  CURRENT_VER=$("$INSTALL_BIN" --version 2>/dev/null | awk '{print $2}' || echo "unknown")
  STATUS_LINE=$(systemctl is-active "$SERVICE_NAME" 2>/dev/null || echo "inactive")
  echo "  当前状态: ✅ 已安装 (v$CURRENT_VER, systemd=$STATUS_LINE)"
else
  echo "  当前状态: 未安装"
fi
echo ""
echo "请选择操作:"
echo "  1. 全新安装 / 重装          (覆盖二进制 + 配置 + 重置 CA)"
echo "  2. 更新版本                 (只换二进制，保留 CA / 数据库 / 配置)"
echo "  3. 重新签发 gRPC server 证书"
echo "  4. 导出 CA bundle           (用于 node 加入)"
echo "  5. 查看面板地址 / 服务状态"
echo "  6. 卸载"
echo "  0. 退出"
echo ""
read -rp "请选择 [0-6]: " CHOICE
case "${CHOICE:-0}" in
  1) ACTION="install" ;;
  2) $IS_INSTALLED || { echo "❌ 还没安装，先选 1"; exit 1; }; ACTION="update" ;;
  3) $IS_INSTALLED || { echo "❌ 还没安装"; exit 1; }; ACTION="regen-server-cert" ;;
  4) $IS_INSTALLED || { echo "❌ 还没安装"; exit 1; }; ACTION="show-ca" ;;
  5) $IS_INSTALLED || { echo "❌ 还没安装"; exit 1; }; ACTION="status" ;;
  6) $IS_INSTALLED || { echo "❌ 还没安装"; exit 0; }; ACTION="uninstall" ;;
  0) exit 0 ;;
  *) echo "无效选项"; exit 1 ;;
esac

# Detect & offer to disable old v0 single-binary services that occupy
# panel/grpc ports and confuse the operator. Run on every action (not
# just install) so the warning surfaces even at menu level.
V0_UNITS=()
for u in relay-rs.service relay-rs-master.service relay-rs-node.service; do
  if systemctl list-unit-files "$u" 2>/dev/null | grep -q "^$u"; then
    V0_UNITS+=("$u")
  fi
done
if (( ${#V0_UNITS[@]} > 0 )); then
  echo ""
  echo "⚠️  检测到旧版 v0 服务仍在系统里（与 v2 不兼容，会占用 9090/19090 等端口）："
  printf '     • %s\n' "${V0_UNITS[@]}"
  read -rp "   停止并禁用？[Y/n]: " _v0
  if [[ "${_v0,,}" != "n" ]]; then
    for u in "${V0_UNITS[@]}"; do
      systemctl disable --now "$u" 2>/dev/null || true
      rm -f "/etc/systemd/system/$u"
    done
    rm -f /usr/local/bin/relay-rs
    systemctl daemon-reload
    echo "   ✅ 旧 v0 已清理"
  fi
fi

# Confirm reinstall on top of an existing install
if $IS_INSTALLED && [[ "$ACTION" == "install" ]]; then
  echo ""
  echo "⚠️  检测到已安装，重装会:"
  echo "   • 覆盖二进制 + env 文件"
  echo "   • 重置 CA（旧 node 证书将作废，需要重新加入）"
  echo "   • 数据库不动"
  read -rp "继续？[y/N]: " _c
  [[ "${_c,,}" != "y" ]] && { echo "已取消"; exit 0; }
  systemctl stop "$SERVICE_NAME" 2>/dev/null || true
  rm -rf "$CA_DIR"
fi

# ── Architecture / version / proxy ───────────────────────────────
ARCH=$(uname -m)
case "$ARCH" in
  x86_64)        TRIPLE="x86_64-unknown-linux-musl" ;;
  aarch64|arm64) TRIPLE="aarch64-unknown-linux-musl" ;;
  *) echo "Unsupported architecture: $ARCH"; exit 1 ;;
esac

if [[ -z "${VERSION:-}" ]]; then
  echo "▶ Resolving latest release..."
  VERSION=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" \
    | grep '"tag_name"' | head -1 | cut -d'"' -f4)
  [[ -z "$VERSION" ]] && { echo "Could not resolve latest version"; exit 1; }
fi

_PROXY_DEFAULT="https://gh-proxy.org/"
if [[ -z "${GITHUB_PROXY+x}" ]]; then
  if curl -fsSL --connect-timeout 5 --max-time 8 -o /dev/null "https://github.com" 2>/dev/null; then
    GITHUB_PROXY=""
  else
    echo "GitHub direct connection slow, using proxy $_PROXY_DEFAULT"
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

# ── status ───────────────────────────────────────────────────────
if [[ "$ACTION" == "status" ]]; then
  set -a; source "$ENV_FILE"; set +a
  PUB_IP=$(curl -fsSL --connect-timeout 5 https://api.ipify.org 2>/dev/null || echo "<this-host>")
  PORT_PANEL="${RELAY_PANEL_LISTEN##*:}"
  PORT_GRPC="${RELAY_MASTER_LISTEN##*:}"
  echo ""
  echo "面板地址:    http://$PUB_IP:${PORT_PANEL:-9090}"
  echo "gRPC 端点:   $PUB_IP:${PORT_GRPC:-9443}"
  echo "服务状态:    $(systemctl is-active "$SERVICE_NAME")"
  echo ""
  systemctl status "$SERVICE_NAME" --no-pager -n 5 2>/dev/null || true
  exit 0
fi

# ── show-ca ──────────────────────────────────────────────────────
if [[ "$ACTION" == "show-ca" ]]; then
  RELAY_MASTER_CA_DIR="$CA_DIR" "$INSTALL_BIN" ca-show --base64 > "$CONFIG_DIR/relay-ca.b64"
  chmod 600 "$CONFIG_DIR/relay-ca.b64"
  echo ""
  echo "✅ CA bundle (base64) written to $CONFIG_DIR/relay-ca.b64"
  echo ""
  echo "Node-side install command:"
  echo "  bash <(curl -fsSL https://raw.githubusercontent.com/$REPO/main/scripts/install-node.sh) \\"
  echo "    --master https://<master-host>:$GRPC_PORT \\"
  echo "    --ca-b64 \"\$(cat $CONFIG_DIR/relay-ca.b64)\" \\"
  echo "    --enrollment-token <token from: relay-master node-add --name X> \\"
  echo "    --node-name <name>"
  exit 0
fi

# ── regen-server-cert ────────────────────────────────────────────
if [[ "$ACTION" == "regen-server-cert" ]]; then
  read -rp "New SAN list (comma separated, e.g. master.example.com,1.2.3.4): " NEW_SAN
  [[ -z "$NEW_SAN" ]] && { echo "SAN must not be empty"; exit 1; }
  sed -i.bak -E "s|^RELAY_MASTER_HOSTNAME=.*|RELAY_MASTER_HOSTNAME=$NEW_SAN|" "$ENV_FILE"
  rm -f "$CA_DIR/server.pem" "$CA_DIR/server.key"
  systemctl restart "$SERVICE_NAME"
  sleep 2
  echo "✅ Server cert regenerated (SAN: $NEW_SAN)"
  exit 0
fi

# ── uninstall ────────────────────────────────────────────────────
if [[ "$ACTION" == "uninstall" ]]; then
  echo ""
  echo "⚠️  卸载将执行："
  echo "    必删:"
  echo "      • systemctl disable --now relay-master"
  echo "      • $SERVICE_FILE"
  echo "      • $INSTALL_BIN"
  echo "      • $ENV_FILE"
  echo "      • $CA_DIR/        (含 CA 私钥、server cert、所有 enrollment tokens)"
  echo "      • $CONFIG_DIR/relay-ca.b64"
  echo ""
  echo "    可选 (会再问):"
  echo "      • docker 容器 $PG_CONTAINER + volume ${PG_CONTAINER}-data"
  echo "      • 外部 Postgres 上的 \`$PG_DB\` 数据库（如果走的是已有 pg）"
  echo ""
  echo "    不删:"
  echo "      • /usr/local/bin/relay-node 及其 systemd unit（属于 node 角色）"
  echo "      • $CONFIG_DIR/ 目录本身（可能还有 node 的 env）"
  echo ""
  read -rp "继续？[y/N]: " _confirm
  [[ "${_confirm,,}" != "y" ]] && { echo "已取消"; exit 0; }

  # Capture DATABASE_URL before we wipe the env file (needed for optional drop-db)
  PRE_DB_URL=""
  [[ -f "$ENV_FILE" ]] && PRE_DB_URL=$(grep -E '^DATABASE_URL=' "$ENV_FILE" 2>/dev/null | head -n1 | cut -d= -f2-)

  systemctl disable --now "$SERVICE_NAME" 2>/dev/null || true
  rm -f "$SERVICE_FILE" "$INSTALL_BIN" "$ENV_FILE" "$CONFIG_DIR/relay-ca.b64"
  rm -rf "$CA_DIR"
  systemctl daemon-reload

  if command -v docker >/dev/null 2>&1 && docker inspect "$PG_CONTAINER" >/dev/null 2>&1; then
    read -rp "删除 docker 容器 $PG_CONTAINER + volume？[y/N]: " _pg
    if [[ "${_pg,,}" == "y" ]]; then
      docker rm -f "$PG_CONTAINER" 2>/dev/null || true
      docker volume rm "${PG_CONTAINER}-data" 2>/dev/null || true
      echo "  ✅ docker pg 已清理"
    fi
  elif [[ -n "$PRE_DB_URL" ]]; then
    read -rp "也从外部 Postgres 上删除 \`$PG_DB\` 数据库 + \`$PG_USER\` 用户？[y/N]: " _drop
    if [[ "${_drop,,}" == "y" ]]; then
      ADMIN_HOSTPORT=$(printf '%s' "$PRE_DB_URL" | sed -E 's|.*@([^/?]+).*|\1|')
      read -rp "  pg 管理员账号 [postgres]: " _au; ADMIN_USER="${_au:-postgres}"
      read -rsp "  $ADMIN_USER 密码 (留空跳过): " ADMIN_PASS; echo
      if [[ -z "$ADMIN_PASS" ]]; then
        echo "  · 跳过外部 pg 清理"
      else
        ADMIN_URL="postgresql://$ADMIN_USER:$ADMIN_PASS@$ADMIN_HOSTPORT/postgres"
        psql_run() {
          if command -v psql >/dev/null 2>&1; then psql "$@"
          else docker run --rm -i --network host postgres:16-alpine psql "$@"; fi
        }
        psql_run "$ADMIN_URL" -c "DROP DATABASE IF EXISTS \"$PG_DB\";" 2>/dev/null || true
        psql_run "$ADMIN_URL" -c "DROP USER IF EXISTS \"$PG_USER\";" 2>/dev/null || true
        echo "  ✅ 外部 pg 上 $PG_DB / $PG_USER 已删除"
      fi
    fi
  fi

  # If /etc/relay-rs is empty after our cleanup, remove it too.
  rmdir "$CONFIG_DIR" 2>/dev/null && echo "  · $CONFIG_DIR 已为空，一并清理"

  echo "✅ relay-master 已卸载"
  exit 0
fi

# ── update ───────────────────────────────────────────────────────
if [[ "$ACTION" == "update" ]]; then
  download_bin "relay-master-$TRIPLE" "$INSTALL_BIN"
  systemctl restart "$SERVICE_NAME"
  echo "✅ Update complete (version $VERSION)"
  exit 0
fi

# ── install ──────────────────────────────────────────────────────

# SAN auto-detection
if [[ -z "${HOSTNAME:-}" ]]; then
  PUB_IP=$(curl -fsSL --connect-timeout 5 https://api.ipify.org 2>/dev/null || true)
  HOSTNAME="127.0.0.1,localhost${PUB_IP:+,$PUB_IP}"
fi
echo "▶ gRPC server cert SAN: $HOSTNAME"

# ── Postgres setup ───────────────────────────────────────────────
NEEDS_DOCKER=false
PG_OK=false

# Fresh install path explicitly nukes any leftover relay-postgres container
# AND its volume so docker init runs cleanly with our freshly generated password.
# Without this, an old volume keeps the previous password and POSTGRES_PASSWORD
# is silently ignored on subsequent runs.
if command -v docker >/dev/null 2>&1 && [[ -z "${DATABASE_URL:-}" ]]; then
  if docker inspect "$PG_CONTAINER" >/dev/null 2>&1 || docker volume inspect "${PG_CONTAINER}-data" >/dev/null 2>&1; then
    echo "▶ 检测到旧的 $PG_CONTAINER 容器/volume —— 全新安装会清理它们"
    read -rp "  继续清理并重建？[Y/n]: " _wipe
    if [[ "${_wipe,,}" != "n" ]]; then
      docker rm -f "$PG_CONTAINER" >/dev/null 2>&1 || true
      docker volume rm "${PG_CONTAINER}-data" >/dev/null 2>&1 || true
    fi
  fi
fi

# Returns 0 if we can actually connect to the given DATABASE_URL (TCP + psql).
check_db_url() {
  local url="$1"
  local hp h p
  hp=$(printf '%s' "$url" | sed -E 's|.*@([^/?]+).*|\1|')
  h="${hp%:*}"; p="${hp##*:}"
  [[ -z "$h" || -z "$p" ]] && return 1
  # Cheap TCP probe first.
  if ! timeout 3 bash -c "exec 3<>/dev/tcp/$h/$p" 2>/dev/null; then
    return 1
  fi
  # Auth probe via psql if available; otherwise via dockerized psql.
  if command -v psql >/dev/null 2>&1; then
    psql "$url" -tAc 'SELECT 1' >/dev/null 2>&1
  elif command -v docker >/dev/null 2>&1; then
    docker run --rm --network host postgres:16-alpine psql "$url" -tAc 'SELECT 1' >/dev/null 2>&1
  else
    # No way to verify auth, accept the TCP probe.
    return 0
  fi
}

if [[ -n "${DATABASE_URL:-}" ]]; then
  if check_db_url "$DATABASE_URL"; then
    echo "▶ Reusing reachable DATABASE_URL"
    PG_OK=true
  fi
fi

# Recover DATABASE_URL from a previous (interrupted) install
if ! $PG_OK && [[ -z "${DATABASE_URL:-}" && -f "$ENV_FILE" ]]; then
  PREV_URL=$(grep -E '^DATABASE_URL=' "$ENV_FILE" 2>/dev/null | head -n1 | cut -d= -f2-)
  if [[ -n "$PREV_URL" ]]; then
    if check_db_url "$PREV_URL"; then
      DATABASE_URL="$PREV_URL"
      echo "▶ Reusing DATABASE_URL from $ENV_FILE"
      PG_OK=true
    else
      echo "  · 旧 env 里的 DATABASE_URL 不可达 ($PREV_URL)，将重新 provision"
    fi
  fi
fi

if ! $PG_OK; then
  if command -v docker >/dev/null 2>&1; then
    echo "▶ Provisioning Postgres via Docker..."
    NEEDS_DOCKER=true
    if docker inspect "$PG_CONTAINER" >/dev/null 2>&1; then
      echo "  · container '$PG_CONTAINER' already exists, reusing"
      docker start "$PG_CONTAINER" >/dev/null 2>&1 || true
      # Recover credentials from the container's env (we set them at create time)
      PG_USER_FROM_CT=$(docker inspect -f '{{range .Config.Env}}{{println .}}{{end}}' "$PG_CONTAINER" \
        | sed -n 's/^POSTGRES_USER=//p' | head -n1)
      PG_PASS_FROM_CT=$(docker inspect -f '{{range .Config.Env}}{{println .}}{{end}}' "$PG_CONTAINER" \
        | sed -n 's/^POSTGRES_PASSWORD=//p' | head -n1)
      PG_DB_FROM_CT=$(docker inspect -f '{{range .Config.Env}}{{println .}}{{end}}' "$PG_CONTAINER" \
        | sed -n 's/^POSTGRES_DB=//p' | head -n1)
      PG_PORT_FROM_CT=$(docker inspect -f '{{(index (index .NetworkSettings.Ports "5432/tcp") 0).HostPort}}' "$PG_CONTAINER" 2>/dev/null || echo "5432")
      if [[ -n "$PG_USER_FROM_CT" && -n "$PG_PASS_FROM_CT" && -n "$PG_DB_FROM_CT" ]]; then
        DATABASE_URL="postgresql://$PG_USER_FROM_CT:$PG_PASS_FROM_CT@127.0.0.1:$PG_PORT_FROM_CT/$PG_DB_FROM_CT"
        echo "  · recovered DATABASE_URL from container env"
      else
        echo "❌ Could not recover credentials from container '$PG_CONTAINER'."
        echo "   Either remove it (docker rm -f $PG_CONTAINER) or pass DATABASE_URL=... and rerun."
        exit 1
      fi
    else
      # Pick a free host port — if 5432 is busy, ask the operator instead
      # of silently spinning up a second postgres on a different port.
      if ss -tln 2>/dev/null | awk '{print $4}' | grep -qE "[:.]5432\$"; then
        echo "  · port 5432 is already serving Postgres — will provision into it."
        echo "    Provide an admin connection URL; the installer will CREATE USER + CREATE DATABASE."
        read -rp "Admin URL (e.g. postgresql://postgres:PASS@127.0.0.1:5432/postgres): " ADMIN_URL
        [[ -z "$ADMIN_URL" ]] && { echo "admin URL required"; exit 1; }

        # psql shim: prefer host binary, fall back to dockerized psql (host network)
        psql_run() {
          if command -v psql >/dev/null 2>&1; then
            psql "$@"
          else
            docker run --rm -i --network host postgres:16-alpine psql "$@"
          fi
        }

        if ! psql_run "$ADMIN_URL" -v ON_ERROR_STOP=1 -c 'SELECT 1' >/dev/null 2>&1; then
          echo "❌ Cannot connect with admin URL"; exit 1
        fi

        PG_PASS=$(openssl rand -hex 16)
        # CREATE or ALTER user (idempotent), then CREATE DATABASE if missing.
        if psql_run "$ADMIN_URL" -tAc "SELECT 1 FROM pg_roles WHERE rolname='$PG_USER'" 2>/dev/null | grep -q 1; then
          psql_run "$ADMIN_URL" -v ON_ERROR_STOP=1 -c "ALTER USER \"$PG_USER\" WITH PASSWORD '$PG_PASS';" >/dev/null
        else
          psql_run "$ADMIN_URL" -v ON_ERROR_STOP=1 -c "CREATE USER \"$PG_USER\" WITH PASSWORD '$PG_PASS';" >/dev/null
        fi
        if ! psql_run "$ADMIN_URL" -tAc "SELECT 1 FROM pg_database WHERE datname='$PG_DB'" 2>/dev/null | grep -q 1; then
          psql_run "$ADMIN_URL" -v ON_ERROR_STOP=1 -c "CREATE DATABASE \"$PG_DB\" OWNER \"$PG_USER\";" >/dev/null
        fi

        # Reuse host:port from admin URL for the final connection string.
        ADMIN_HOSTPORT=$(printf '%s' "$ADMIN_URL" | sed -E 's|.*@([^/?]+).*|\1|')
        DATABASE_URL="postgresql://$PG_USER:$PG_PASS@$ADMIN_HOSTPORT/$PG_DB"
        echo "  · provisioned $PG_DB / $PG_USER on existing Postgres"
        NEEDS_DOCKER=false
        PG_OK=true
      else
        PG_PASS=$(openssl rand -hex 16)
        docker volume create "${PG_CONTAINER}-data" >/dev/null
        if ! docker run -d --name "$PG_CONTAINER" \
          --restart unless-stopped \
          -e POSTGRES_DB="$PG_DB" \
          -e POSTGRES_USER="$PG_USER" \
          -e POSTGRES_PASSWORD="$PG_PASS" \
          -p 127.0.0.1:5432:5432 \
          -v "${PG_CONTAINER}-data:/var/lib/postgresql/data" \
          postgres:16-alpine >/dev/null; then
          echo "❌ Failed to start Postgres container."
          docker rm -f "$PG_CONTAINER" >/dev/null 2>&1 || true
          exit 1
        fi
        echo "  · waiting for Postgres to become ready..."
        for _ in $(seq 1 30); do
          if docker exec "$PG_CONTAINER" pg_isready -U "$PG_USER" -d "$PG_DB" >/dev/null 2>&1; then
            break
          fi
          sleep 1
        done
        # Reused volumes keep their initial password; force-sync it via the
        # container's local unix socket (peer-trust) so we don't get locked out.
        docker exec "$PG_CONTAINER" psql -U "$PG_USER" -d "$PG_DB" \
          -c "ALTER USER \"$PG_USER\" WITH PASSWORD '$PG_PASS';" >/dev/null 2>&1 || true
        DATABASE_URL="postgresql://$PG_USER:$PG_PASS@127.0.0.1:5432/$PG_DB"
      fi
    fi
    PG_OK=true
  else
    echo "⚠️  Docker not available and no working DATABASE_URL detected."
    read -rp "Please enter DATABASE_URL (e.g. postgresql://user:pass@host:5432/relay): " DATABASE_URL
    [[ -z "$DATABASE_URL" ]] && { echo "DATABASE_URL required"; exit 1; }
  fi
fi

download_bin "relay-master-$TRIPLE" "$INSTALL_BIN"

mkdir -p "$CONFIG_DIR" "$CA_DIR"

JWT_SECRET=$(openssl rand -hex 32)

# ── 面板对外 URL（必填，Discourse SSO 校验依赖它）────────────────
echo
echo "─── 面板对外 URL（必填）───"
echo "Discourse 会按这里的 host 去匹配 discourse_connect_provider_secrets。"
echo "必须跟你 Discourse 后台那一行 \`<host>|<secret>\` 左边的域名一致。"
echo "示例：https://panel.example.com  （末尾不要带斜杠）"
PANEL_EXTERNAL_URL=""
while [[ -z "$PANEL_EXTERNAL_URL" ]]; do
  read -rp "  面板对外 URL: " PANEL_EXTERNAL_URL
  if [[ ! "$PANEL_EXTERNAL_URL" =~ ^https?:// ]]; then
    echo "  ❌ 必须以 http:// 或 https:// 开头"
    PANEL_EXTERNAL_URL=""
  fi
done
PANEL_EXTERNAL_URL="${PANEL_EXTERNAL_URL%/}"

cat > "$ENV_FILE" <<ENV
# relay-master configuration
RELAY_MASTER_CA_DIR=$CA_DIR
RELAY_MASTER_TOKEN_DIR=$CA_DIR/enrollment-tokens
RELAY_MASTER_LISTEN=0.0.0.0:$GRPC_PORT
RELAY_MASTER_HOSTNAME=$HOSTNAME
DATABASE_URL=$DATABASE_URL
RELAY_PANEL_LISTEN=0.0.0.0:$PANEL_PORT
RELAY_PANEL_EXTERNAL_URL=$PANEL_EXTERNAL_URL
# 32-byte hex secret. Rotating it logs out every panel session.
RELAY_PANEL_JWT_SECRET=$JWT_SECRET
RUST_LOG=info
ENV
chmod 600 "$ENV_FILE"

# ── systemd unit ─────────────────────────────────────────────────
{
  echo "[Unit]"
  echo "Description=relay-master control plane (gRPC over mTLS)"
  if $NEEDS_DOCKER; then
    echo "After=network-online.target docker.service"
    echo "Wants=network-online.target"
    echo "Requires=docker.service"
  else
    echo "After=network-online.target"
    echo "Wants=network-online.target"
  fi
  echo ""
  echo "[Service]"
  echo "EnvironmentFile=$ENV_FILE"
  echo "ExecStart=$INSTALL_BIN daemon"
  echo "Restart=on-failure"
  echo "RestartSec=5"
  echo "LimitNOFILE=65536"
  echo ""
  echo "[Install]"
  echo "WantedBy=multi-user.target"
} > "$SERVICE_FILE"

systemctl daemon-reload
systemctl enable --now "$SERVICE_NAME"
sleep 2

if ! systemctl is-active --quiet "$SERVICE_NAME"; then
  echo "❌ relay-master failed to start:"
  journalctl -u "$SERVICE_NAME" -n 40 --no-pager
  exit 1
fi

# ── Discourse SSO bootstrap (required) ───────────────────────────
echo ""
echo "─── Discourse SSO 配置（必填） ───"
echo "面板登录唯一走 Discourse Connect。不配完整就装完了也登不进面板。"
echo "在你的 Discourse 后台 Admin → Settings → Login 启用 enable_discourse_connect_provider，"
echo "并设置 discourse_connect_provider_secrets，secret 在那里生成。"
echo ""
DISCOURSE_URL=""
DISCOURSE_SECRET=""
while [[ -z "$DISCOURSE_URL" || -z "$DISCOURSE_SECRET" ]]; do
  read -rp "  Discourse 站点 URL (如 https://forum.example.com): " DISCOURSE_URL
  if [[ -z "$DISCOURSE_URL" ]]; then
    echo "    ❌ URL 不能为空"
    continue
  fi
  if [[ ! "$DISCOURSE_URL" =~ ^https?:// ]]; then
    echo "    ❌ 必须以 http:// 或 https:// 开头"
    DISCOURSE_URL=""; continue
  fi
  read -rsp "  Discourse SSO secret (输入不显示，回车确认): " DISCOURSE_SECRET
  echo
  if [[ -z "$DISCOURSE_SECRET" ]]; then
    echo "    ❌ secret 不能为空"
    DISCOURSE_URL=""; continue
  fi
done

if ! ( set -a; . "$ENV_FILE"; set +a; \
       printf '%s' "$DISCOURSE_SECRET" | "$INSTALL_BIN" discourse-set --url "$DISCOURSE_URL" ); then
  echo "❌ 写入 Discourse 配置失败，install 中止"
  exit 1
fi
unset DISCOURSE_SECRET
echo "  ✅ Discourse SSO 已配置（master 30s 内自动重载）"

# ── Export CA bundle ─────────────────────────────────────────────
CA_B64=$(RELAY_MASTER_CA_DIR="$CA_DIR" "$INSTALL_BIN" ca-show --base64)
echo "$CA_B64" > "$CONFIG_DIR/relay-ca.b64"
chmod 600 "$CONFIG_DIR/relay-ca.b64"

PUB_IP="${PUB_IP:-<master-host>}"

echo ""
echo "✅ relay-master installed (version $VERSION)"
echo ""
echo "Endpoints:"
echo "  · Web panel : http://$PUB_IP:$PANEL_PORT  (terminate TLS at a reverse proxy)"
echo "  · gRPC mTLS : https://$PUB_IP:$GRPC_PORT  (node ingress)"
echo "  · 对外 URL  : $PANEL_EXTERNAL_URL"
echo ""
echo "Add a node:"
echo "  1) On this host, generate a one-time enrollment token:"
echo "       $INSTALL_BIN node-add --name <node-name>"
echo ""
echo "  2) On the node host:"
echo "       bash <(curl -fsSL https://raw.githubusercontent.com/$REPO/main/scripts/install-node.sh) \\"
echo "         --master https://$PUB_IP:$GRPC_PORT \\"
echo "         --ca-b64 \"\$(cat $CONFIG_DIR/relay-ca.b64)\" \\"
echo "         --enrollment-token <token from step 1> \\"
echo "         --node-name <node-name>"
echo ""
echo "CA bundle (base64) cached at $CONFIG_DIR/relay-ca.b64"
echo ""
echo "Common commands:"
echo "  systemctl status $SERVICE_NAME          service status"
echo "  $INSTALL_BIN node-add --name X          generate enrollment token"
echo "  $INSTALL_BIN ca-show --base64           re-export CA bundle"
echo "  journalctl -u $SERVICE_NAME -f          follow logs"
