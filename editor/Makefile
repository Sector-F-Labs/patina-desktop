TARGET := $(CURDIR)/../target
.DEFAULT_GOAL := help

APP_ID := com.sectorflabs.TextEditor
PREFIX ?= $(HOME)/.local
BINDIR := $(PREFIX)/bin
APPSDIR := $(PREFIX)/share/applications
ICONDIR := $(PREFIX)/share/icons/hicolor

# A throwaway config and data dir, so test runs never touch your settings,
# session or drafts, and run as a window of their own (ADR-0005).
SCRATCH_ENV := XDG_CONFIG_HOME=$(TARGET)/scratch-config XDG_DATA_HOME=$(TARGET)/scratch-data

.PHONY: help run dev build test check icons icon install uninstall screenshot clean

help: ## Show this help
	@echo "Usage: make <target>"
	@echo
	@awk 'BEGIN { FS = ":.*## " } /^[a-z-]+:.*## / { printf "  \033[1m%-14s\033[0m %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

run: ## Run on this repository's README, in a scratch config
	@mkdir -p $(TARGET)/scratch-config $(TARGET)/scratch-data
	$(SCRATCH_ENV) cargo run -p text-editor -- README.md

dev: ## Run with live reload (requires cargo-watch)
	@command -v cargo-watch >/dev/null 2>&1 || { echo "cargo-watch is required: cargo install cargo-watch"; exit 1; }
	@mkdir -p $(TARGET)/scratch-config $(TARGET)/scratch-data
	$(SCRATCH_ENV) cargo watch -x "run -p text-editor -- README.md"

build: ## Build a release binary
	cargo build -p text-editor --release

test: ## Run the tests
	cargo test -p text-editor

check: test ## Run the tests and clippy
	cargo clippy -p text-editor --all-targets -- -D warnings

icons: ## Render assets/text-editor-128.png from packaging/linux/text-editor.svg
	cargo run -p text-editor -q --example render_icon -- 128 assets/text-editor-128.png

icon: ## docs/icon.png: the app icon at 120 px, for the README
	scripts/icon.sh

# For the current user; no sudo. The launcher's Exec gets the absolute path,
# so it works even if $(BINDIR) isn't on the session's PATH.
install: build ## Install Text Editor and its launcher for the current user (PREFIX=~/.local)
	install -Dm755 $(TARGET)/release/text-editor $(BINDIR)/text-editor
	install -Dm644 packaging/linux/text-editor.svg $(ICONDIR)/scalable/apps/$(APP_ID).svg
	install -Dm644 assets/text-editor-128.png $(ICONDIR)/128x128/apps/$(APP_ID).png
	install -d $(APPSDIR)
	sed 's|^Exec=text-editor|Exec=$(BINDIR)/text-editor|' packaging/linux/$(APP_ID).desktop > $(APPSDIR)/$(APP_ID).desktop
	-update-desktop-database -q $(APPSDIR)
	@# GTK trusts an icon cache over the folder, so refresh one if it's there.
	-@test ! -f $(ICONDIR)/icon-theme.cache || gtk-update-icon-cache -q -t -f $(ICONDIR)
	@echo "Installed $(BINDIR)/text-editor and the $(APP_ID) launcher"

uninstall: ## Remove what make install put in place
	rm -f $(BINDIR)/text-editor $(APPSDIR)/$(APP_ID).desktop
	rm -f $(ICONDIR)/scalable/apps/$(APP_ID).svg $(ICONDIR)/128x128/apps/$(APP_ID).png
	-update-desktop-database -q $(APPSDIR)
	-@test ! -f $(ICONDIR)/icon-theme.cache || gtk-update-icon-cache -q -t -f $(ICONDIR)

screenshot: build ## Regenerate docs/screenshot-*.png from src/find.rs, in a scratch config
	@rm -rf $(TARGET)/scratch-config $(TARGET)/scratch-data
	@mkdir -p docs $(TARGET)/scratch-config $(TARGET)/scratch-data
	$(SCRATCH_ENV) TEXT_EDITOR_SCREENSHOT=docs/screenshot-dark.png $(TARGET)/release/text-editor src/find.rs
	@rm -rf $(TARGET)/scratch-data
	$(SCRATCH_ENV) TEXT_EDITOR_SCREENSHOT=docs/screenshot-find.png TEXT_EDITOR_SKIN=Notebook TEXT_EDITOR_FIND=query TEXT_EDITOR_REPLACE=1 $(TARGET)/release/text-editor src/find.rs

clean: ## Remove build output
	cargo clean -p text-editor
