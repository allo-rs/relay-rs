# relay-rs

基于 [nftables](https://nftables.org/) 的 NAT 端口转发守护进程，用 Rust 编写。

支持将本机端口流量动态转发到远端地址，自动检测 DNS 变化并实时更新规则。

## 系统要求

- Linux（内核 ≥ 4.10，支持 nftables）
- 已安装 `nftables`（包含 `/usr/sbin/nft`）
- root 权限

```bash
# Debian / Ubuntu
apt install nftables

# CentOS / RHEL
dnf install nftables
```

## 安装

### 一键安装（推荐）

```bash
# 国内服务器（走代理）
bash <(curl -fsSL https://mirror.ghproxy.com/https://raw.githubusercontent.com/allo-rs/relay-rs/main/install.sh)

# 境外服务器（直连）
bash <(curl -fsSL https://raw.githubusercontent.com/allo-rs/relay-rs/main/install.sh)
```

自动完成：检测架构 → 下载最新二进制 → 创建配置模板 → 安装 systemd 服务。

### 从源码编译

```bash
cargo build --release
sudo cp target/release/relay-rs /usr/local/bin/
```

## 配置

默认配置文件路径：`/etc/relay-rs/relay.toml`

```bash
sudo mkdir -p /etc/relay-rs
sudo cp relay.toml.example /etc/relay-rs/relay.toml
sudo vim /etc/relay-rs/relay.toml
```

### 配置格式

通过 `type` 字段选择规则类型。

#### 单端口转发（`type = "single"`）

```toml
[[rules]]
type = "single"
sport = 10000          # 本机监听端口
dport = 443            # 目标端口
target = "example.com" # 目标域名或 IP（支持 IPv4/IPv6）
protocol = "tcp"       # tcp | udp | all（默认 all）
ip_version = "ipv4"    # ipv4 | ipv6 | all（默认 ipv4）
comment = "可选备注"   # 会写入 nftables 规则注释
```

#### 端口段转发（`type = "range"`）

```toml
[[rules]]
type = "range"
sport_start = 10000    # 本机端口段起始
sport_end = 10100      # 本机端口段结束
dport_start = 20000    # 目标端口段起始（省略则与 sport_start 相同）
target = "10.0.0.1"
protocol = "tcp"
ip_version = "ipv4"
comment = "端口段 10000-10100 → 10.0.0.1:20000-20100"
```

## 用法

```bash
# 使用默认配置文件启动
sudo relay-rs

# 指定配置文件
sudo relay-rs --config /path/to/relay.toml

# 自定义轮询间隔（秒，默认 60）
sudo relay-rs --interval 30

# 查看帮助
relay-rs --help
```

### 日志级别

通过环境变量 `RUST_LOG` 控制：

```bash
# 默认 info 级别
sudo relay-rs

# 开启 debug 日志（显示每次规则比对详情）
sudo RUST_LOG=debug relay-rs
```

## systemd 服务

创建 `/etc/systemd/system/relay-rs.service`：

```ini
[Unit]
Description=relay-rs NAT forwarding daemon
After=network.target

[Service]
ExecStart=/usr/local/bin/relay-rs --config /etc/relay-rs/relay.toml
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now relay-rs
sudo systemctl status relay-rs
```

## 工作原理

1. 启动时自动开启内核 IP 转发（`ip_forward`）
2. 读取 `relay.toml`，对每条规则进行 DNS 解析
3. 生成 nftables 脚本并执行（写入 `/etc/relay-rs/rules.nft`）
4. 每隔 `--interval` 秒重新解析，若 IP 或配置变化则自动更新规则

生成的 nftables 规则示例：

```
add rule ip relay-nat PREROUTING ct state new tcp dport 10000 counter dnat to 93.184.216.34:443
add rule ip relay-nat POSTROUTING ct state new ip daddr 93.184.216.34 tcp dport 443 counter masquerade
```

## 查看当前规则

```bash
# 查看 relay-rs 生成的规则
sudo nft list table ip relay-nat
sudo nft list table ip6 relay-nat

# 或直接查看规则脚本
cat /etc/relay-rs/rules.nft
```
