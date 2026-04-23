CURRENT := $(shell grep '^version' Cargo.toml | head -1 | cut -d'"' -f2)
MAJOR   := $(shell echo $(CURRENT) | cut -d. -f1)
MINOR   := $(shell echo $(CURRENT) | cut -d. -f2)
PATCH   := $(shell echo $(CURRENT) | cut -d. -f3)

NEXT_PATCH := $(MAJOR).$(MINOR).$(shell expr $(PATCH) + 1)
NEXT_MINOR := $(MAJOR).$(shell expr $(MINOR) + 1).0
NEXT_MAJOR := $(shell expr $(MAJOR) + 1).0.0

BINARY  := ./target/debug/relay-rs
DEV_CFG := dev.toml

.PHONY: build build-panel dev dev-setup clean release _do_release _build_debug

# 构建前端（bun）
build-panel:
	cd panel && bun install --frozen-lockfile && bun run build

# 构建完整发布二进制（先构建前端再嵌入）
build: build-panel
	cargo build --release

# 首次初始化开发配置（文件已存在则跳过，防止重复覆盖）
$(DEV_CFG): $(BINARY)
	@if [ -f $(DEV_CFG) ]; then \
	  touch $(DEV_CFG); \
	else \
	  echo "初始化开发配置 $(DEV_CFG)..."; \
	  printf 'mode = "relay"\n\n[panel]\nmode = "master"\nlisten = "127.0.0.1:9090"\nsecret = "dev-jwt-secret-change-me"\n# TODO: 填写真实数据库地址\ndatabase_url = "postgresql://USER:PASS@HOST:5432/relay?sslmode=disable"\n' > $(DEV_CFG); \
	  echo "⚠️  请编辑 $(DEV_CFG) 填写真实的 database_url，然后重新运行 make dev-setup"; \
	  echo ""; \
	  echo "初始化面板 Ed25519 密钥:"; \
	  $(BINARY) --config $(DEV_CFG) panel-init; \
	fi

dev-setup: $(BINARY) $(DEV_CFG)

# 本地开发：后端(:9090) + Vite 前端(:5173) 同时启动
# Ctrl+C 会同时终止两个进程
dev: _build_debug $(DEV_CFG)
	@echo "▶ 面板后端  http://127.0.0.1:9090"
	@echo "▶ 前端 Dev  http://127.0.0.1:5173  (代理 /api → 后端)"
	@echo "Ctrl+C 退出\n"
	@trap 'kill 0' SIGINT SIGTERM EXIT; \
	  RUST_LOG=info $(BINARY) --config $(DEV_CFG) daemon & \
	  (cd panel && bun run dev) & \
	  wait

# 每次都检查 Rust 源码变更并重编 debug 二进制
_build_debug:
	cargo build

$(BINARY):
	cargo build

# 清理构建产物（保留 dev.toml）
clean:
	cargo clean
	rm -rf panel/dist panel/node_modules

release:
	@echo "当前版本: $(CURRENT)"
	@echo ""
	@echo "  1) patch  →  $(NEXT_PATCH)"
	@echo "  2) minor  →  $(NEXT_MINOR)"
	@echo "  3) major  →  $(NEXT_MAJOR)"
	@echo ""
	@read -p "选择 [1/2/3]: " choice; \
	case "$$choice" in \
		1) $(MAKE) _do_release NEXT=$(NEXT_PATCH) ;; \
		2) $(MAKE) _do_release NEXT=$(NEXT_MINOR) ;; \
		3) $(MAKE) _do_release NEXT=$(NEXT_MAJOR) ;; \
		*) echo "已取消" ;; \
	esac

_do_release:
	@echo "$(CURRENT) → $(NEXT)"
	@sed -i '' 's/^version = "$(CURRENT)"/version = "$(NEXT)"/' Cargo.toml
	@git add Cargo.toml
	@git commit -m "chore: bump version to $(NEXT)"
	@git tag -a v$(NEXT) -m "Release v$(NEXT)"
	@git push origin main v$(NEXT)
