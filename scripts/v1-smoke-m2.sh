#!/usr/bin/env bash
# M2 本地烟雾测试：启动 master → 生成 token → node 注册 → 验证 cert
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

WORKDIR="$(mktemp -d /tmp/relay-v1-smoke.XXXXXX)"
echo "▶ workdir: $WORKDIR"
trap 'kill $MASTER_PID 2>/dev/null || true; echo "(master killed)"' EXIT

export RELAY_MASTER_CA_DIR="$WORKDIR/master"
export RELAY_MASTER_TOKEN_DIR="$WORKDIR/master/tokens"
export RELAY_MASTER_LISTEN="127.0.0.1:29443"
export RELAY_MASTER_HOSTNAME="127.0.0.1,localhost"
export RUST_LOG="info"

echo "▶ build"
cargo build --release -p relay-master -p relay-node 2>&1 | tail -2

MASTER_BIN="$ROOT/target/release/relay-master"
NODE_BIN="$ROOT/target/release/relay-node"

echo "▶ 启动 master daemon"
"$MASTER_BIN" daemon > "$WORKDIR/master.log" 2>&1 &
MASTER_PID=$!
echo "  pid=$MASTER_PID, log=$WORKDIR/master.log"
sleep 2
if ! kill -0 "$MASTER_PID" 2>/dev/null; then
  echo "❌ master 未启动"; cat "$WORKDIR/master.log"; exit 1
fi

echo "▶ 生成 enrollment token"
TOKEN=$("$MASTER_BIN" node-add --name node-alpha | awk '/enrollment token:/ {print $3}')
[[ -z "$TOKEN" ]] && { echo "❌ token 生成失败"; exit 1; }
echo "  token=${TOKEN:0:16}..."

echo "▶ 导出 CA bundle"
CA_B64=$("$MASTER_BIN" ca-show --base64)
[[ -z "$CA_B64" ]] && { echo "❌ CA 导出失败"; exit 1; }
echo "  ca b64 len=${#CA_B64}"

echo "▶ node 注册"
export NODE_STATE_DIR="$WORKDIR/node-alpha"
MASTER_ADDR="https://127.0.0.1:29443" \
MASTER_CA_PEM_B64="$CA_B64" \
ENROLLMENT_TOKEN="$TOKEN" \
NODE_NAME="node-alpha" \
"$NODE_BIN" register 2>&1 | tee "$WORKDIR/node-register.log"

echo "▶ 验证产物"
for f in node.pem node.key ca.pem node_id ca.bundle_version; do
  p="$NODE_STATE_DIR/$f"
  [[ -f "$p" ]] || { echo "❌ 缺 $p"; exit 1; }
  echo "  ✓ $f ($(stat -f%z "$p" 2>/dev/null || stat -c%s "$p") bytes)"
done

echo "▶ 验证 cert 由 CA 签发"
if command -v openssl >/dev/null; then
  openssl verify -CAfile "$NODE_STATE_DIR/ca.pem" "$NODE_STATE_DIR/node.pem"
fi

echo "▶ 验证 token 已被消费（重放应失败）"
REPLAY_OUT=$(MASTER_ADDR="https://127.0.0.1:29443" \
   MASTER_CA_PEM_B64="$CA_B64" \
   ENROLLMENT_TOKEN="$TOKEN" \
   NODE_NAME="node-alpha" \
   NODE_STATE_DIR="$WORKDIR/node-replay" \
   "$NODE_BIN" register 2>&1 || true)
if echo "$REPLAY_OUT" | grep -q "token 不存在\|已被消费\|enrollment token 无效"; then
  echo "  ✓ 重放正确被拒"
else
  echo "❌ token 可被重放，一次性语义失效"
  echo "$REPLAY_OUT"
  exit 1
fi

echo ""
echo "✅ M2 烟雾测试通过"
