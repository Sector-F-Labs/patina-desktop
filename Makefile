.DEFAULT_GOAL := help

APP_ID := com.sectorflabs.Reader
PREFIX ?= $(HOME)/.local
BINDIR := $(PREFIX)/bin
APPSDIR := $(PREFIX)/share/applications
ICONDIR := $(PREFIX)/share/icons/hicolor

# Throwaway config, so test runs never touch yours.
SCRATCH := target/scratch

.PHONY: help run build test check install uninstall screenshot icon clean

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
	XDG_CONFIG_HOME=$(CURDIR)/$(SCRATCH) READER_SCREENSHOT=docs/screenshot-markdown.png target/release/reader README.md

icon: ## docs/icon.png: the app icon at 120 px, for the README
	scripts/icon.sh

# For the current user; no sudo.
install: build ## Install Reader and its launcher for the current user (PREFIX=~/.local)
	install -Dm755 target/release/reader $(BINDIR)/reader
	install -Dm644 packaging/linux/reader.svg $(ICONDIR)/scalable/apps/$(APP_ID).svg
	install -d $(APPSDIR)
	sed 's|^Exec=reader|Exec=$(BINDIR)/reader|' packaging/linux/$(APP_ID).desktop > $(APPSDIR)/$(APP_ID).desktop
	-update-desktop-database -q $(APPSDIR)
	-@test ! -f $(ICONDIR)/icon-theme.cache || gtk-update-icon-cache -q -t -f $(ICONDIR)
	@echo "Installed $(BINDIR)/reader and the $(APP_ID) launcher"

uninstall: ## Remove what make install put in place
	rm -f $(BINDIR)/reader $(APPSDIR)/$(APP_ID).desktop $(ICONDIR)/scalable/apps/$(APP_ID).svg
	-update-desktop-database -q $(APPSDIR)

clean: ## Remove build output
	cargo clean
