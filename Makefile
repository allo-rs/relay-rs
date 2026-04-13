CURRENT := $(shell grep '^version' Cargo.toml | head -1 | cut -d'"' -f2)
MAJOR   := $(shell echo $(CURRENT) | cut -d. -f1)
MINOR   := $(shell echo $(CURRENT) | cut -d. -f2)
PATCH   := $(shell echo $(CURRENT) | cut -d. -f3)

NEXT_PATCH := $(MAJOR).$(MINOR).$(shell expr $(PATCH) + 1)
NEXT_MINOR := $(MAJOR).$(shell expr $(MINOR) + 1).0
NEXT_MAJOR := $(shell expr $(MAJOR) + 1).0.0

.PHONY: build release _do_release

build:
	cargo build --release

release:
	@echo "当前版本: $(CURRENT)"
	@echo ""
	@echo "  x) patch  →  $(NEXT_PATCH)"
	@echo "  y) minor  →  $(NEXT_MINOR)"
	@echo "  z) major  →  $(NEXT_MAJOR)"
	@echo ""
	@read -p "选择 [x/y/z]: " choice; \
	case "$$choice" in \
		x) $(MAKE) _do_release NEXT=$(NEXT_PATCH) ;; \
		y) $(MAKE) _do_release NEXT=$(NEXT_MINOR) ;; \
		z) $(MAKE) _do_release NEXT=$(NEXT_MAJOR) ;; \
		*) echo "已取消" ;; \
	esac

_do_release:
	@echo "$(CURRENT) → $(NEXT)"
	@sed -i '' 's/^version = "$(CURRENT)"/version = "$(NEXT)"/' Cargo.toml
	@git add Cargo.toml
	@git commit -m "chore: bump version to $(NEXT)"
	@git tag -a v$(NEXT) -m "Release v$(NEXT)"
	@git push origin main v$(NEXT)
