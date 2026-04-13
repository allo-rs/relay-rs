CURRENT := $(shell grep '^version' Cargo.toml | head -1 | cut -d'"' -f2)
MAJOR   := $(shell echo $(CURRENT) | cut -d. -f1)
MINOR   := $(shell echo $(CURRENT) | cut -d. -f2)
PATCH   := $(shell echo $(CURRENT) | cut -d. -f3)

NEXT_PATCH := $(MAJOR).$(MINOR).$(shell expr $(PATCH) + 1)
NEXT_MINOR := $(MAJOR).$(shell expr $(MINOR) + 1).0
NEXT_MAJOR := $(shell expr $(MAJOR) + 1).0.0

.PHONY: build release release-minor release-major

build:
	cargo build --release

## 发布 patch 版本（0.1.0 → 0.1.1）
release:
	@$(MAKE) _do_release NEXT=$(NEXT_PATCH)

## 发布 minor 版本（0.1.0 → 0.2.0）
release-minor:
	@$(MAKE) _do_release NEXT=$(NEXT_MINOR)

## 发布 major 版本（0.1.0 → 1.0.0）
release-major:
	@$(MAKE) _do_release NEXT=$(NEXT_MAJOR)

_do_release:
	@echo "$(CURRENT) → $(NEXT)"
	@sed -i '' 's/^version = "$(CURRENT)"/version = "$(NEXT)"/' Cargo.toml
	@git add Cargo.toml
	@git commit -m "chore: bump version to $(NEXT)"
	@git tag -a v$(NEXT) -m "Release v$(NEXT)"
	@git push origin main v$(NEXT)
