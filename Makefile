CURRENT := $(shell grep '^version' Cargo.toml | head -1 | cut -d'"' -f2)
MAJOR   := $(shell echo $(CURRENT) | cut -d. -f1)
MINOR   := $(shell echo $(CURRENT) | cut -d. -f2)
PATCH   := $(shell echo $(CURRENT) | cut -d. -f3)

NEXT_PATCH := $(MAJOR).$(MINOR).$(shell expr $(PATCH) + 1)
NEXT_MINOR := $(MAJOR).$(shell expr $(MINOR) + 1).0
NEXT_MAJOR := $(shell expr $(MAJOR) + 1).0.0

BINARY  := ./target/debug/relay-rs
DEV_CFG := dev.toml

.PHONY: build build-panel dev dev-setup clean release _do_release

# 构建前端（bun）
build-panel:
	cd panel && bun install --frozen-lockfile && bun run build

# 构建完整发布二进制（先构建前端再嵌入）
build: build-panel
	cargo build --release

# 首次初始化开发配置
$(DEV_CFG): $(BINARY)
	@echo "初始化开发配置 $(DEV_CFG)..."
	@printf 'mode = "relay"\n\n[panel]\nmode = "master"\nlisten = "127.0.0.1:9090"\nsecret = "dev-jwt-secret-change-me"\ndatabase_url = "postgresql://user:pass@host:5432/relay?sslmode=disable"\n' > $(DEV_CFG)
	@echo ""
	@echo "设置面板管理员密码（用于登录 Web 面板）:"
	@$(BINARY) --config $(DEV_CFG) panel-passwd

dev-setup: $(BINARY) $(DEV_CFG)

# 本地开发：后端(:9090) + Vite 前端(:5173) 同时启动
# Ctrl+C 会同时终止两个进程
dev: $(BINARY) $(DEV_CFG)
	@echo "▶ 面板后端  http://127.0.0.1:9090"
	@echo "▶ 前端 Dev  http://127.0.0.1:5173  (代理 /api → 后端)"
	@echo "Ctrl+C 退出\n"
	@trap 'kill 0' SIGINT SIGTERM EXIT; \
	  RUST_LOG=info $(BINARY) --config $(DEV_CFG) daemon & \
	  (cd panel && bun run dev) & \
	  wait

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
