.DEFAULT_GOAL := help

# The workspace builds into the repository's target/, not the crate's.
TARGET ?= ../target

# Throwaway config, so test runs never touch yours.
SCRATCH := target/scratch

.PHONY: help run build test check screenshot icon clean

help: ## Show this help
	@echo "Usage: make <target>"
	@echo
	@awk 'BEGIN { FS = ":.*## " } /^[a-z-]+:.*## / { printf "  \033[1m%-14s\033[0m %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

run: ## Read this repository
	cargo run -- .

build: ## Build a release binary
	cargo build --release

test: ## Run the tests
	cargo test

check: test ## Run the tests and clippy
	cargo clippy --all-targets -- -D warnings

screenshot: build ## docs/screenshot-markdown.png: this README, in a scratch config
	@mkdir -p $(SCRATCH) docs
	XDG_CONFIG_HOME=$(CURDIR)/$(SCRATCH) READER_SCREENSHOT=docs/screenshot-markdown.png $(TARGET)/release/reader README.md

icon: ## docs/icon.png: the app icon at 120 px, for the README
	scripts/icon.sh

clean: ## Remove build output
	cargo clean
