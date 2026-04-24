# relay-rs

基于 [nftables](https://nftables.org/) 的 NAT 端口转发守护进程，用 Rust 编写。

- 支持 **NAT 模式**（nftables 内核直转）和 **Relay 模式**（用户态 tokio 代理 + splice 零拷贝）
- 自动检测 DNS TTL 变化并实时更新规则
- 内置 Web 管理面板，支持多节点中控

## 系统要求

- Linux（内核 ≥ 4.10，支持 nftables）
- 已安装 `nftables`（NAT 模式必须）
- root 权限

```bash
# Debian / Ubuntu
apt install nftables

# CentOS / RHEL
dnf install nftables
```

> **主控模式**额外需要 Docker（安装脚本会自动安装）。

## 安装

国内服务器（自动检测，走代理）：

```bash
bash <(curl -fsSL https://gh-proxy.org/https://raw.githubusercontent.com/allo-rs/relay-rs/main/install.sh)
```

境外服务器（直连）：

```bash
bash <(curl -fsSL https://raw.githubusercontent.com/allo-rs/relay-rs/main/install.sh)
```

自动完成：检测架构 → 下载最新二进制 → 创建配置模板 → 安装 systemd 服务。

## 管理命令

安装后通过 `rr` 命令管理：

```bash
rr                # 交互式菜单
rr start          # 启动服务
rr stop           # 停止服务
rr restart        # 重启服务
rr status         # 查看状态
rr log            # 实时日志
rr config         # 编辑配置文件
rr reload         # 编辑配置并重启
rr list           # 列出所有规则
rr stats          # 查看各规则流量统计
rr add            # 交互式添加规则
rr del            # 交互式删除规则
rr edit           # 交互式编辑已有规则
rr check          # 检查转发规则连通性
rr ping host:port # 探测指定地址端口
rr mode           # 切换转发模式（nat / relay）
```

## 配置

配置文件路径：`/etc/relay-rs/relay.toml`

### 转发模式

```toml
# nat（默认）：nftables 内核直转，性能最佳
# relay：用户态代理，支持多目标负载均衡和限速
mode = "nat"
```

### 转发规则（`[[forward]]`）

| 字段 | 说明 | 默认 |
|------|------|------|
| `listen` | 本机监听端口，单端口 `10000` 或端口段 `"10000-10100"` | 必填 |
| `to` | 目标地址，单个 `"host:port"` 或多个 `["h1:port", "h2:port"]` | 必填 |
| `proto` | `tcp` \| `udp` \| `all` | `all` |
| `ipv6` | 强制解析 IPv6 | `false` |
| `balance` | 多目标负载均衡策略：`round-robin` \| `random` | `round-robin` |
| `rate_limit` | 带宽限速，单位 Mbps（仅 relay 模式） | 可选 |
| `comment` | 备注 | 可选 |

```toml
# 单端口，TCP + UDP
[[forward]]
listen = 10000
to = "example.com:443"

# 只转发 TCP
[[forward]]
listen = 8080
to = "10.0.0.1:80"
proto = "tcp"
comment = "HTTP 中转"

# 端口段：本机 10000-10100 → 目标 20000-20100
[[forward]]
listen = "10000-10100"
to = "10.0.0.1:20000"

# 多目标负载均衡（relay 模式）
[[forward]]
listen = 10000
to = ["node1.example.com:443", "node2.example.com:443"]
balance = "round-robin"
rate_limit = 200   # 限速 200 Mbps

# 强制 IPv6
[[forward]]
listen = 10001
to = "example.com:443"
ipv6 = true
```

### 防火墙规则（`[[block]]`）

| 字段 | 说明 | 默认 |
|------|------|------|
| `src` | 源 IP 或 CIDR | 可选 |
| `dst` | 目标 IP 或 CIDR | 可选 |
| `port` | 目标端口 | 可选 |
| `proto` | `tcp` \| `udp` \| `all` | `all` |
| `chain` | `input` \| `forward` | `input` |
| `ipv6` | 匹配 IPv6 | `false` |

```toml
# 封禁单个 IP
[[block]]
src = "1.2.3.4"
comment = "封禁扫描 IP"

# 封禁整个网段
[[block]]
src = "192.168.100.0/24"

# 禁止外部访问数据库
[[block]]
port = 3306
proto = "tcp"
comment = "禁止外部访问 MySQL"

# 封禁转发链上的 IP 段
[[block]]
src = "10.0.0.0/8"
chain = "forward"
```

## Web 管理面板

relay-rs 支持可选的 Web 管理面板，采用主控（master）+ 被控（node）架构。

- **node**：在每台中转机上运行，暴露 API 供 master 调用
- **master**：运行 Web 面板，聚合管理所有 node 的转发规则，需要 PostgreSQL

### 安装主控

```bash
# 国内（自动走代理）
bash <(curl -fsSL https://gh-proxy.org/https://raw.githubusercontent.com/allo-rs/relay-rs/main/scripts/install-master.sh)

# 境外（直连）
bash <(curl -fsSL https://raw.githubusercontent.com/allo-rs/relay-rs/main/scripts/install-master.sh)
```

脚本全自动完成：安装 Docker → 启动 PostgreSQL（自动生成随机密码）→ 下载二进制 → 初始化密钥 → 注册 systemd 服务。运行过程中仅询问面板端口（默认 9090）。

安装完成后访问 `http://<server-ip>:9090`，首次为开放模式，在「设置 → Discourse 接入」配置 SSO 后启用登录验证。

### 安装节点

在主控面板添加节点后，直接复制弹出的安装命令（已内嵌主控公钥），在节点机器上以 root 执行：

```bash
bash <(curl -fsSL https://raw.githubusercontent.com/allo-rs/relay-rs/main/scripts/install-node.sh) \
  --port 9090 --pubkey-b64 <主控公钥 base64>
```

### 手动配置（可选）

**Node 模式**（`/etc/relay-rs/relay.toml`）：

```toml
[panel]
mode = "node"
listen = "0.0.0.0:9090"
master_pubkey = """
-----BEGIN PUBLIC KEY-----
...
-----END PUBLIC KEY-----"""
```

**Master 模式**：所有配置存于 PostgreSQL，启动参数通过 `/etc/relay-rs/env` 注入：

```bash
# /etc/relay-rs/env（由安装脚本自动生成，权限 600）
DATABASE_URL=postgresql://relay:PASS@127.0.0.1:5432/relay?sslmode=disable
PANEL_LISTEN=0.0.0.0:9090
```

```bash
# Discourse SSO 配置锁死时恢复开放模式
rr panel-reset-auth
```

### 面板功能

- **节点管理**：添加/删除被控节点，查看在线状态
- **转发规则**：图形化增删改转发规则，支持跨节点聚合视图
- **防火墙规则**：图形化管理 block 规则
- **流量统计**：实时查看各规则的 bytes in/out 和连接数

## 工作原理

**NAT 模式**：

1. 启动时自动开启内核 IP 转发（`ip_forward`）
2. 读取转发规则（node 模式从 `relay.toml`，master 模式从 PostgreSQL），对规则进行 DNS 解析
3. 生成 nftables 脚本并执行（写入 `/etc/relay-rs/rules.nft`）
4. 根据 DNS TTL（最短 15s，最长 300s）自动轮询，IP 变化时更新规则

**Relay 模式**：

- 用户态 tokio 异步代理，Linux 上使用 `splice(2)` 零拷贝转发
- 支持多目标负载均衡和带宽限速
- 规则变化时热重载，无需重建 nftables 规则

## 查看当前规则

```bash
rr log                           # 实时日志
rr stats                         # 流量统计
nft list table ip relay-nat      # 转发规则（NAT 模式）
nft list table ip relay-filter   # 防火墙规则
cat /etc/relay-rs/rules.nft      # 规则脚本原文
```
