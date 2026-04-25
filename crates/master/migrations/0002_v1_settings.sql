-- v1 面板/控制面通用 KV 设置表（与 v0 settings 完全分离）
-- 当前用途：discourse SSO 配置（key='discourse', value={url, secret}）
CREATE TABLE IF NOT EXISTS v1_settings (
  key        TEXT PRIMARY KEY,
  value      JSONB NOT NULL,
  updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
