.DEFAULT_GOAL := help

PREFIX ?= $(HOME)/.local

.PHONY: help run build test check install uninstall clean

help: ## Show this help
	@echo "Usage: make <target>"
	@echo
	@awk 'BEGIN { FS = ":.*## " } /^[a-z-]+:.*## / { printf "  \033[1m%-14s\033[0m %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

run: ## Open the launcher on this desktop
	cargo run

build: ## Build a release binary
	cargo build --release

test: ## Run the tests
	cargo test

check: test ## Run the tests and clippy
	cargo clippy --all-targets -- -D warnings

install: build ## Install into $(PREFIX)/bin
	install -Dm755 target/release/patina-launcher $(PREFIX)/bin/patina-launcher

uninstall: ## Remove what make install put in place
	rm -f $(PREFIX)/bin/patina-launcher

clean: ## Remove build output
	cargo clean
