-- relay-master v1 初始 schema
-- 全部表/类型加 v1 前缀或后缀，避免与 v0 (src/db.rs ensure_schema 生成的 nodes/forward_rules/block_rules/settings) 冲突。
-- v0 和 v1 在过渡期共享同一个数据库；v1.1 起 v0 表会被 drop。

-- ── 枚举 ────────────────────────────────────────────────────────
DO $$ BEGIN
  CREATE TYPE v1_proto AS ENUM ('tcp', 'udp', 'all');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
  CREATE TYPE v1_balance AS ENUM ('round_robin', 'random', 'source_hash');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
  CREATE TYPE v1_next_kind AS ENUM ('node', 'upstream');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

DO $$ BEGIN
  CREATE TYPE v1_node_status AS ENUM ('pending', 'ok', 'degraded', 'offline');
EXCEPTION WHEN duplicate_object THEN NULL; END $$;

-- ── v1 nodes：从 mTLS 注册来的节点 ─────────────────────────────
-- 与 v0 `nodes`（SERIAL id, name, url）完全分离
CREATE TABLE IF NOT EXISTS v1_nodes (
  id                    TEXT        PRIMARY KEY,              -- master 签发时生成的 UUID (node-<uuid>)
  name                  TEXT        NOT NULL,                  -- 运维可读名
  public_ip             TEXT,                                  -- 由 master 根据接入来源填（首次连 Sync 时落定）
  desired_revision      BIGINT      NOT NULL DEFAULT 0,        -- 单调递增；只在「该 node 相关规则变更」时 +1
  applied_revision      BIGINT      NOT NULL DEFAULT 0,        -- node 最后一次成功 apply 的 revision
  desired_hash          BYTEA       NOT NULL DEFAULT ''::bytea, -- master 计算的当前 desired envelope hash
  actual_hash           BYTEA       NOT NULL DEFAULT ''::bytea, -- node 回报的实际生效 hash（可能与 desired 不同）
  status                v1_node_status NOT NULL DEFAULT 'pending',
  last_seen             TIMESTAMPTZ,                           -- 最后一次收到 Hello/Heartbeat 的时间
  conn_gen              BIGINT      NOT NULL DEFAULT 0,        -- 服务端分配的连接代数；所有 Ack/状态写回必须 CAS 本字段
  session_epoch         BIGINT      NOT NULL DEFAULT 0,        -- node 上报的本地重启计数（仅日志）
  version               TEXT,                                  -- node 二进制版本
  ca_bundle_version     INT         NOT NULL DEFAULT 0,        -- node 侧当前持有的 CA bundle 版本
  enrolled_at           TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  updated_at            TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS v1_nodes_status_idx     ON v1_nodes(status);
CREATE INDEX IF NOT EXISTS v1_nodes_last_seen_idx  ON v1_nodes(last_seen);

-- ── v1 segments：中继链的最小单元（列式 next，不用 JSON 藏关系） ──
CREATE TABLE IF NOT EXISTS v1_segments (
  id                    TEXT        PRIMARY KEY,               -- 稳定 UUID
  chain_id              TEXT        NOT NULL,                  -- 归属 chain（UI 聚合用）
  listen_node_id        TEXT        NOT NULL REFERENCES v1_nodes(id) ON DELETE CASCADE,
  listen                TEXT        NOT NULL,                  -- "80" / "80-100"
  proto                 v1_proto    NOT NULL,
  ipv6                  BOOLEAN     NOT NULL DEFAULT FALSE,

  -- next 拆为列，CHECK 保证 oneof 语义
  next_kind             v1_next_kind NOT NULL,
  next_segment_id       TEXT        REFERENCES v1_segments(id) ON DELETE RESTRICT,
  upstream_host         TEXT,
  upstream_port_start   INT,
  upstream_port_end     INT,

  rate_limit_mbps       INT,
  balance               v1_balance  NOT NULL DEFAULT 'round_robin',
  comment               TEXT,
  updated_at            TIMESTAMPTZ NOT NULL DEFAULT NOW(),

  CONSTRAINT v1_segments_next_chk CHECK (
    (next_kind = 'node'
      AND next_segment_id IS NOT NULL
      AND upstream_host IS NULL
      AND upstream_port_start IS NULL
      AND upstream_port_end IS NULL)
    OR
    (next_kind = 'upstream'
      AND next_segment_id IS NULL
      AND upstream_host IS NOT NULL
      AND upstream_port_start IS NOT NULL
      AND upstream_port_end IS NOT NULL
      AND upstream_port_end >= upstream_port_start)
  ),
  CONSTRAINT v1_segments_port_range CHECK (
    (upstream_port_start IS NULL OR upstream_port_start BETWEEN 1 AND 65535)
    AND
    (upstream_port_end   IS NULL OR upstream_port_end   BETWEEN 1 AND 65535)
  )
);

CREATE INDEX IF NOT EXISTS v1_segments_listen_node_idx ON v1_segments(listen_node_id);
CREATE INDEX IF NOT EXISTS v1_segments_chain_idx       ON v1_segments(chain_id);
CREATE INDEX IF NOT EXISTS v1_segments_next_seg_idx    ON v1_segments(next_segment_id);

-- ── v1 apply_errors：node 侧报告的 per-segment 应用错误 ────────
CREATE TABLE IF NOT EXISTS v1_apply_errors (
  node_id       TEXT NOT NULL REFERENCES v1_nodes(id) ON DELETE CASCADE,
  segment_id    TEXT NOT NULL,                                 -- 不加 FK：node 报错时 segment 可能已被删
  revision      BIGINT NOT NULL,                               -- 报错那一次的 revision
  message       TEXT NOT NULL,
  reported_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
  PRIMARY KEY (node_id, segment_id, revision)
);

-- ── v1 enrollment_tokens：托底，未来迁出文件落盘（M2 是文件，M3+ 可切 DB） ──
-- 先建表占位，让运维能在一个地方看全；master 二进制暂仍从文件读
CREATE TABLE IF NOT EXISTS v1_enrollment_tokens (
  token_hash    BYTEA PRIMARY KEY,                             -- sha256(token)；明文只回给运维一次
  node_name     TEXT NOT NULL,
  expires_at    TIMESTAMPTZ NOT NULL,
  consumed_at   TIMESTAMPTZ,
  created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- ── v1 schema_info：简单的 schema version 记录（与 sqlx 迁移表并存） ──
CREATE TABLE IF NOT EXISTS v1_schema_info (
  key         TEXT PRIMARY KEY,
  value       TEXT NOT NULL,
  updated_at  TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
INSERT INTO v1_schema_info(key, value) VALUES ('schema_version', '1')
  ON CONFLICT (key) DO NOTHING;

-- ── NOTIFY channel：master reconciler 监听它感知变更 ─────────
-- 使用方：UPDATE v1_nodes SET desired_revision = desired_revision + 1 WHERE id = ?;
--        SELECT pg_notify('v1_node_desired_changed', id) FROM v1_nodes WHERE id = ?;
-- （触发器写起来更优雅但初版先放业务层）

COMMENT ON TABLE  v1_nodes     IS 'relay-rs v1 数据面节点（mTLS 身份）';
COMMENT ON COLUMN v1_nodes.conn_gen IS 'server-assigned conn generation；Ack/状态写回必须 CAS 本字段防旧流残留';
COMMENT ON TABLE  v1_segments  IS 'v1 中继链最小单元；next 拆列避免 JSON 藏关系';
