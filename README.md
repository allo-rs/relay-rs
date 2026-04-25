# relay-rs

`relay-rs` 是一套集中式 TCP/UDP 中继系统。`relay-master` 控制面（Postgres 存储、mTLS gRPC、axum 管理面板 + Discourse SSO）将期望状态推送给一个或多个 `relay-node` 数据面；节点负责接受客户端连接并转发至上游。

```
            ┌────────────────────┐
            │   relay-master     │  控制面
            │  管理面板   :9090  │  (Postgres, JWT/SSO)
            │  gRPC mTLS  :9443  │
            └─────────┬──────────┘
                      │  下发 segment 配置 / 心跳
                      ▼
   ┌───────────┐  ┌───────────┐  ┌───────────┐
   │ relay-node│  │ relay-node│  │ relay-node│   数据面
   └───────────┘  └───────────┘  └───────────┘   (TCP/UDP 中继)
```

## 快速开始

### 1. 安装 master

```bash
curl -fsSL https://raw.githubusercontent.com/allo-rs/relay-rs/main/scripts/install-master.sh | bash
```

安装脚本会下载 `relay-master` 二进制、自动创建 Postgres 数据库（通过 Docker，或复用已有的 `DATABASE_URL`）、生成 CA、写入 `/etc/relay-rs/relay-master.env`，并启动 `relay-master.service`。

### 2. 添加节点

在 master 主机上生成一次性注册 token：

```bash
relay-master node-add --name edge-1
```

在节点主机上执行：

```bash
bash <(curl -fsSL https://raw.githubusercontent.com/allo-rs/relay-rs/main/scripts/install-node.sh) \
  --master https://master.example.com:9443 \
  --ca-b64 "$(cat /etc/relay-rs/relay-ca.b64)" \
  --enrollment-token <node-add 输出的 token> \
  --node-name edge-1
```

### 3. 打开管理面板

访问 `http://<master>:9090`（生产环境建议在反向代理处终止 TLS，并相应设置 `RELAY_PANEL_EXTERNAL_URL`）。

## 架构

- **`relay-master`** — 控制面。以 Postgres 为单一数据源，对节点暴露 gRPC mTLS API，并提供管理面板。面板通过 Discourse SSO 认证，使用 JWT 签发会话。
- **`relay-node`** — 数据面。注册时获取 mTLS 客户端证书，持续与 master 同步期望状态，并绑定 TCP/UDP 监听端口将流量转发至上游。
- **`relay-proto`** — 两个二进制共用的 protobuf / tonic 桩代码。

## 配置

`relay-master` 从 `/etc/relay-rs/relay-master.env` 读取配置：

| 变量 | 说明 |
| --- | --- |
| `DATABASE_URL` | Postgres 连接字符串（必填） |
| `RELAY_MASTER_CA_DIR` | CA 及服务端证书目录 |
| `RELAY_MASTER_TOKEN_DIR` | 注册 token 存放目录 |
| `RELAY_MASTER_LISTEN` | gRPC mTLS 监听地址（如 `0.0.0.0:9443`） |
| `RELAY_MASTER_HOSTNAME` | 服务端证书 SAN 列表，逗号分隔 |
| `RELAY_PANEL_LISTEN` | 管理面板 HTTP 监听地址（如 `0.0.0.0:9090`） |
| `RELAY_PANEL_EXTERNAL_URL` | 面板对外访问的公网 URL |
| `RELAY_PANEL_JWT_SECRET` | 32 字节 hex 密钥，用于签发面板会话 |
| `RELAY_MASTER_PUBLIC_URL` | 可选；覆盖面板向节点通告的 master 地址 |

`relay-node` 从 `/etc/relay-rs/relay-node.env` 读取配置：

| 变量 | 说明 |
| --- | --- |
| `MASTER_ADDR` | master 的 gRPC 地址（如 `https://master:9443`） |
| `NODE_STATE_DIR` | 节点证书及状态文件存放目录 |
| `MASTER_CA_PEM_B64` | （仅注册时）master CA 的 base64 PEM |
| `ENROLLMENT_TOKEN` | （仅注册时）`node-add` 生成的一次性 token |
| `NODE_NAME` | （仅注册时）节点可读名称 |

## 开发

```bash
# 编译全部 crate
cargo build --workspace

# 对生产 crate 执行 clippy
cargo clippy -p relay-proto -p relay-master -p relay-node -- -D warnings

# 单元测试
cargo test --workspace

# 应用 SQL 迁移到本地 Postgres
for f in crates/master/migrations/*.sql; do
  psql "$DATABASE_URL" -v ON_ERROR_STOP=1 -f "$f"
done

# 构建前端（需要 Bun）
cd panel && bun install && bun run build
```

端到端 smoke 测试（未设置 `DATABASE_URL` 时自动在 Docker 中启动 Postgres，随后运行 master + node 并验证 TCP 转发）：

```bash
bash scripts/smoke.sh
```

## License

MIT
