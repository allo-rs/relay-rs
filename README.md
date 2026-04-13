# relay-rs

基于 [nftables](https://nftables.org/) 的 NAT 端口转发守护进程，用 Rust 编写。

支持将本机端口流量动态转发到远端地址，自动检测 DNS 变化并实时更新规则。

## 系统要求

- Linux（内核 ≥ 4.10，支持 nftables）
- 已安装 `nftables`
- root 权限

```bash
# Debian / Ubuntu
apt install nftables

# CentOS / RHEL
dnf install nftables
```

## 安装

国内服务器（走代理）：

```bash
bash <(curl -fsSL https://gh-proxy.org/https://raw.githubusercontent.com/allo-rs/relay-rs/main/install.sh)
```

境外服务器（直连）：

```bash
bash <(curl -fsSL https://raw.githubusercontent.com/allo-rs/relay-rs/main/install.sh)
```

自动完成：检测架构 → 下载最新二进制 → 创建配置模板 → 安装 systemd 服务。

## 配置

配置文件路径：`/etc/relay-rs/relay.toml`

### 转发规则（`[[forward]]`）

| 字段 | 说明 | 默认 |
|------|------|------|
| `listen` | 本机监听端口，单端口 `10000` 或端口段 `"10000-10100"` | 必填 |
| `to` | 目标地址，格式 `"host:port"` | 必填 |
| `proto` | `tcp` \| `udp` \| `all` | `all` |
| `ipv6` | 强制解析 IPv6 | `false` |
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

# 强制 IPv6
[[forward]]
listen = 10000
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

## 管理命令

安装后通过 `rr` 命令管理：

```bash
rr stats    # 查看各规则流量统计
rr start    # 启动服务
rr stop     # 停止服务
rr restart  # 重启服务
rr status   # 查看状态
rr log      # 实时日志
rr config   # 编辑配置
rr reload   # 编辑配置并重启
```

## 工作原理

1. 启动时自动开启内核 IP 转发（`ip_forward`）
2. 读取 `relay.toml`，对转发规则进行 DNS 解析
3. 生成 nftables 脚本并执行（写入 `/etc/relay-rs/rules.nft`）
4. 每隔 60 秒重新检测，若 IP 或配置变化则自动更新规则

生成的 nftables 规则示例：

```
# 转发
add rule ip relay-nat PREROUTING ct state new tcp dport 10000 counter dnat to 93.184.216.34:443
add rule ip relay-nat POSTROUTING ct state new ip daddr 93.184.216.34 tcp dport 443 counter masquerade

# 防火墙
add rule ip relay-filter INPUT ip saddr 1.2.3.4 drop
```

## 查看当前规则

```bash
rr log                      # 实时日志
nft list table ip relay-nat      # 转发规则
nft list table ip relay-filter   # 防火墙规则
cat /etc/relay-rs/rules.nft      # 规则脚本原文
```
