VERSION ?= $(shell grep '^version' Cargo.toml | head -1 | cut -d'"' -f2)

.PHONY: build release tag

build:
	cargo build --release

release: tag
	git push origin v$(VERSION)

tag:
	git tag -a v$(VERSION) -m "Release v$(VERSION)"
