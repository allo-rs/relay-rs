#!/usr/bin/env bash
# M3 本地烟雾测试：PG + master + node + FullSync 端到端验收
#
# 前置：
#   - docker 可用（自动启一个 postgres:16 容器），或
#   - 设置 DATABASE_URL 指向已有 PG（会在其中创建 v1_* 表；清理只做 DROP）
#
# 验收点：
#   1. master 启动，迁移跑完
#   2. node Register 成功（Register 无 mTLS client cert）
#   3. 往 v1_segments 插一条 TCP→upstream 段
#   4. node daemon 启动 Sync → 收到 FullSync → bind 监听 → Ack
#   5. 用 nc 建立端到端 TCP 连通
#   6. v1_nodes.status='ok' 且 desired_hash == actual_hash

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

WORKDIR="$(mktemp -d /tmp/relay-v1-m3-smoke.XXXXXX)"
echo "▶ workdir: $WORKDIR"

LISTEN_PORT=29443
SEG_LISTEN_PORT=22080          # node 对外监听
UPSTREAM_PORT=22081            # 假 upstream（nc -l）
SEG_ID="seg-test-$(date +%s)"
NODE_NAME="node-smoke"

# ── PostgreSQL 准备 ───────────────────────────────────────────
PG_CONTAINER=""
if [[ -z "${DATABASE_URL:-}" ]]; then
  if ! command -v docker >/dev/null; then
    echo "❌ 需要 DATABASE_URL 或 docker 启 postgres，请安装其中之一"
    exit 2
  fi
  PG_CONTAINER="relay-v1-m3-pg-$$"
  echo "▶ 启动 PostgreSQL 容器 $PG_CONTAINER"
  docker run -d --rm --name "$PG_CONTAINER" \
    -e POSTGRES_PASSWORD=relay -e POSTGRES_USER=relay -e POSTGRES_DB=relay \
    -p 25432:5432 postgres:16 >/dev/null
  export DATABASE_URL="postgresql://relay:relay@127.0.0.1:25432/relay"
  echo "  等待 PG 可用..."
  for i in $(seq 1 30); do
    if docker exec "$PG_CONTAINER" pg_isready -U relay >/dev/null 2>&1; then
      break
    fi
    sleep 1
  done
fi
echo "  DATABASE_URL=${DATABASE_URL//relay:*@/relay:***@}"

# ── 清理函数 ────────────────────────────────────────────────
cleanup() {
  set +e
  [[ -n "${MASTER_PID:-}" ]] && kill "$MASTER_PID" 2>/dev/null
  [[ -n "${NODE_PID:-}" ]] && kill "$NODE_PID" 2>/dev/null
  [[ -n "${NC_PID:-}" ]] && kill "$NC_PID" 2>/dev/null
  if [[ -n "$PG_CONTAINER" ]]; then
    docker rm -f "$PG_CONTAINER" >/dev/null 2>&1
  fi
  echo "(已清理)"
}
trap cleanup EXIT

# ── 选 psql 客户端 ────────────────────────────────────────────
psql_run() {
  if command -v psql >/dev/null; then
    psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -q -A -t "$@"
  elif [[ -n "$PG_CONTAINER" ]]; then
    docker exec -e PGPASSWORD=relay "$PG_CONTAINER" psql -U relay -d relay -v ON_ERROR_STOP=1 -q -A -t "$@"
  else
    echo "❌ 需要本机 psql 或 docker 容器 psql" >&2
    return 1
  fi
}

# ── 构建 ─────────────────────────────────────────────────────
echo "▶ cargo build release"
if ! cargo build --release -p relay-master -p relay-node 2>&1 | tail -80; then
  echo "FAIL: cargo build failed"
  exit 1
fi
MASTER_BIN="$ROOT/target/release/relay-master"
NODE_BIN="$ROOT/target/release/relay-node"

# ── 启动 master ─────────────────────────────────────────────
export RELAY_MASTER_CA_DIR="$WORKDIR/master"
export RELAY_MASTER_TOKEN_DIR="$WORKDIR/master/tokens"
export RELAY_MASTER_LISTEN="127.0.0.1:$LISTEN_PORT"
export RELAY_MASTER_HOSTNAME="127.0.0.1,localhost"
export RUST_LOG="${RUST_LOG:-info}"

echo "▶ 启动 master daemon"
"$MASTER_BIN" daemon > "$WORKDIR/master.log" 2>&1 &
MASTER_PID=$!
for i in $(seq 1 15); do
  if grep -q "gRPC 监听" "$WORKDIR/master.log" 2>/dev/null; then break; fi
  sleep 1
done
if ! kill -0 "$MASTER_PID" 2>/dev/null; then
  echo "❌ master 未启动"; cat "$WORKDIR/master.log"; exit 1
fi

# ── 注册 node ────────────────────────────────────────────────
echo "▶ 生成 enrollment token"
TOKEN=$("$MASTER_BIN" node-add --name "$NODE_NAME" | awk '/enrollment token:/ {print $3}')
[[ -z "$TOKEN" ]] && { echo "❌ token 生成失败"; exit 1; }

CA_B64=$("$MASTER_BIN" ca-show --base64)

echo "▶ node register"
export NODE_STATE_DIR="$WORKDIR/node"
MASTER_ADDR="https://127.0.0.1:$LISTEN_PORT" \
MASTER_CA_PEM_B64="$CA_B64" \
ENROLLMENT_TOKEN="$TOKEN" \
NODE_NAME="$NODE_NAME" \
"$NODE_BIN" register > "$WORKDIR/node-register.log" 2>&1
NODE_ID=$(cat "$NODE_STATE_DIR/node_id")
echo "  ✓ node_id=$NODE_ID"

# ── 插入一条 segment 到 DB ────────────────────────────────────
echo "▶ 写 v1_segments 一条 TCP → 127.0.0.1:$UPSTREAM_PORT"
psql_run <<SQL
INSERT INTO v1_segments(id, chain_id, listen_node_id, listen, proto, ipv6,
                       next_kind, upstream_host, upstream_port_start, upstream_port_end,
                       balance)
VALUES ('$SEG_ID', 'chain-smoke', '$NODE_ID', '$SEG_LISTEN_PORT', 'tcp', FALSE,
        'upstream', '127.0.0.1', $UPSTREAM_PORT, $UPSTREAM_PORT,
        'round_robin');
UPDATE v1_nodes SET desired_revision = 1 WHERE id = '$NODE_ID';
SQL

# ── 启一个假 upstream（echo 一行退出） ───────────────────────
echo "▶ 起 nc 假 upstream on :$UPSTREAM_PORT"
{ echo "hello-from-upstream"; sleep 5; } | nc -l "$UPSTREAM_PORT" > "$WORKDIR/upstream-received.txt" &
NC_PID=$!
sleep 0.3

# ── 启 node daemon ──────────────────────────────────────────
echo "▶ 启动 node daemon"
MASTER_ADDR="https://127.0.0.1:$LISTEN_PORT" \
NODE_STATE_DIR="$NODE_STATE_DIR" \
"$NODE_BIN" daemon > "$WORKDIR/node.log" 2>&1 &
NODE_PID=$!

# 等 node 拿到 FullSync 并 bind
echo "  等 FullSync → bind on :$SEG_LISTEN_PORT"
for i in $(seq 1 20); do
  if grep -q "listening on" "$WORKDIR/node.log" 2>/dev/null; then break; fi
  sleep 1
done
grep -q "listening on" "$WORKDIR/node.log" || {
  echo "❌ node 未成功 bind"; tail -40 "$WORKDIR/node.log"; exit 1;
}
echo "  ✓ node bind 成功"

# ── 端到端连通性 ────────────────────────────────────────────
echo "▶ 验证 TCP 经过 node 到 upstream"
ANS=$(echo "ping" | nc -w 2 127.0.0.1 "$SEG_LISTEN_PORT" || true)
if [[ "$ANS" != "hello-from-upstream" ]]; then
  echo "❌ 转发未通，收到：'$ANS'"; tail -40 "$WORKDIR/node.log"; exit 1
fi
echo "  ✓ 收到：$ANS"

# ── DB 状态断言 ─────────────────────────────────────────────
sleep 1   # 给 Ack 往返一点时间
STATUS=$(psql_run -c "SELECT status FROM v1_nodes WHERE id='$NODE_ID'" | tr -d ' ')
DH=$(psql_run -c "SELECT encode(desired_hash,'hex') FROM v1_nodes WHERE id='$NODE_ID'" | tr -d ' ')
AH=$(psql_run -c "SELECT encode(actual_hash,'hex')  FROM v1_nodes WHERE id='$NODE_ID'" | tr -d ' ')
echo "  status=$STATUS"
echo "  desired_hash=$DH"
echo "  actual_hash =$AH"

if [[ "$STATUS" != "ok" ]]; then
  echo "❌ 期望 status=ok，实际 $STATUS"; exit 1
fi
if [[ -z "$DH" || "$DH" != "$AH" ]]; then
  echo "❌ hash 不一致（desired != actual）"; exit 1
fi

# ── Reconciler push 验证：使用 seg-add CLI 加第二段，期望 node 几秒内 bind ────
SEG2_LISTEN_PORT=22090
UPSTREAM2_PORT=22091
echo ""
echo "▶ 起第二个 nc 假 upstream on :$UPSTREAM2_PORT（keep-alive 循环）"
# 每次收到连接就 echo 一句再关闭，下次连接重开（覆盖 nc -z 探测 + 真实转发两次握手）
(
  for _ in 1 2 3 4 5; do
    echo "hello-from-upstream-2" | nc -l "$UPSTREAM2_PORT" 2>/dev/null || true
  done
) > "$WORKDIR/upstream2-received.txt" 2>&1 &
UP2_PID=$!
sleep 0.3

echo "▶ 通过 seg-add CLI 增加第二段（触发 LISTEN/NOTIFY → reconciler 重推）"
DATABASE_URL="$DATABASE_URL" "$MASTER_BIN" seg-add \
  --node "$NODE_ID" \
  --listen "0.0.0.0:$SEG2_LISTEN_PORT" \
  --upstream "127.0.0.1:$UPSTREAM2_PORT"

echo "  等 reconciler 推送 + node 重 bind + 转发（最多 15s）"
ANS2=""
for i in $(seq 1 30); do
  ANS2=$(echo "ping" | nc -w 2 127.0.0.1 "$SEG2_LISTEN_PORT" 2>/dev/null || true)
  if [[ "$ANS2" == "hello-from-upstream-2" ]]; then
    echo "  ✓ seg2 转发成功（约 $((i*500))ms）：$ANS2"
    break
  fi
  if (( i == 30 )); then
    echo "❌ reconciler 推送/转发超时（最后一次收到 '$ANS2'）"
    tail -60 "$WORKDIR/node.log"
    exit 1
  fi
  sleep 0.5
done

# 等 Ack 回到 master
sleep 1
STATUS2=$(psql_run -c "SELECT status FROM v1_nodes WHERE id='$NODE_ID'" | tr -d ' ')
DH2=$(psql_run -c "SELECT encode(desired_hash,'hex') FROM v1_nodes WHERE id='$NODE_ID'" | tr -d ' ')
AH2=$(psql_run -c "SELECT encode(actual_hash,'hex')  FROM v1_nodes WHERE id='$NODE_ID'" | tr -d ' ')
echo "  seg2 后 status=$STATUS2"
if [[ "$STATUS2" != "ok" || "$DH2" != "$AH2" ]]; then
  echo "❌ seg2 后 status/hash 不一致：status=$STATUS2 dh=$DH2 ah=$AH2"
  tail -60 "$WORKDIR/node.log"; exit 1
fi

# ── node-list / seg-list CLI 健康检查 ────────────────────────
echo ""
echo "▶ 验证 node-list / seg-list CLI"
"$MASTER_BIN" node-list | grep -q "$NODE_ID" || { echo "❌ node-list 未显示 $NODE_ID"; exit 1; }
"$MASTER_BIN" seg-list --node "$NODE_ID" | grep -q "$SEG2_LISTEN_PORT" || { echo "❌ seg-list 未显示 seg2"; exit 1; }
echo "  ✓ CLI 输出正确"

echo ""
echo "✅ M3 烟雾测试通过（含 reconciler push）"
