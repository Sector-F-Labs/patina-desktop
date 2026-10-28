.DEFAULT_GOAL := help

APP_ID := com.sectorflabs.Home
PREFIX ?= $(HOME)/.local
BINDIR := $(PREFIX)/bin
APPSDIR := $(PREFIX)/share/applications
ICONDIR := $(PREFIX)/share/icons/hicolor

# Throwaway config, data and cache dirs, so test runs never touch yours.
SCRATCH := target/scratch
SCRATCH_ENV := XDG_CONFIG_HOME=$(CURDIR)/$(SCRATCH)/config XDG_DATA_HOME=$(CURDIR)/$(SCRATCH)/data XDG_CACHE_HOME=$(CURDIR)/$(SCRATCH)/cache

.PHONY: help run dev build test check screenshot icons install uninstall clean

help: ## Show this help
	@echo "Usage: make <target>"
	@echo
	@awk 'BEGIN { FS = ":.*## " } /^[a-z-]+:.*## / { printf "  \033[1m%-14s\033[0m %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

run: ## Run Home with your own settings
	cargo run

dev: ## Run with live reload (requires cargo-watch), in a scratch config
	@command -v cargo-watch >/dev/null 2>&1 || { echo "cargo-watch is required: cargo install cargo-watch"; exit 1; }
	$(SCRATCH_ENV) cargo watch -x "run --bin home"

build: ## Build a release binary
	cargo build --release

test: ## Run the tests
	cargo test

check: test ## Run the tests and clippy, as before a commit or release
	cargo clippy --all-targets -- -D warnings

screenshot: ## Regenerate docs/screenshot-*.png from demo entities, in a scratch config
	@rm -rf $(SCRATCH) && mkdir -p docs $(SCRATCH)/config $(SCRATCH)/data $(SCRATCH)/cache
	$(SCRATCH_ENV) HOME_DEMO=1 HOME_SCREENSHOT=docs/screenshot-default.png cargo run
	$(SCRATCH_ENV) HOME_DEMO=1 HOME_SKIN="Dark Orange" HOME_SCREENSHOT=docs/screenshot-dark-orange.png cargo run
	$(SCRATCH_ENV) HOME_DEMO=1 HOME_OPEN=light.living_room HOME_SCREENSHOT=docs/screenshot-light.png cargo run

icons: ## Render assets/home-icon-128.png from packaging/linux/home.svg
	cargo run -q --example render_icon -- 128 assets/home-icon-128.png

# For the current user; no sudo. The launcher's Exec gets the absolute path.
install: build ## Install Home and its launcher for the current user (PREFIX=~/.local)
	install -Dm755 target/release/home $(BINDIR)/home
	install -Dm644 packaging/linux/home.svg $(ICONDIR)/scalable/apps/$(APP_ID).svg
	install -Dm644 assets/home-icon-128.png $(ICONDIR)/128x128/apps/$(APP_ID).png
	install -d $(APPSDIR)
	sed 's|^Exec=home|Exec=$(BINDIR)/home|' packaging/linux/$(APP_ID).desktop > $(APPSDIR)/$(APP_ID).desktop
	-update-desktop-database -q $(APPSDIR)
	-@test ! -f $(ICONDIR)/icon-theme.cache || gtk-update-icon-cache -q -t -f $(ICONDIR)
	@echo "Installed $(BINDIR)/home and the $(APP_ID) launcher"

uninstall: ## Remove what make install put in place
	rm -f $(BINDIR)/home $(APPSDIR)/$(APP_ID).desktop
	rm -f $(ICONDIR)/scalable/apps/$(APP_ID).svg $(ICONDIR)/128x128/apps/$(APP_ID).png
	-update-desktop-database -q $(APPSDIR)
	-@test ! -f $(ICONDIR)/icon-theme.cache || gtk-update-icon-cache -q -t -f $(ICONDIR)

clean: ## Remove build output
	cargo clean
