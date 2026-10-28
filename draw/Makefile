APP_ID := com.sectorflabs.Draw
PREFIX ?= $(HOME)/.local
BINDIR := $(PREFIX)/bin
APPSDIR := $(PREFIX)/share/applications
ICONDIR := $(PREFIX)/share/icons/hicolor
VERSION := $(shell awk -F\" '/^version = / { print $$2; exit }' Cargo.toml)

# A throwaway config and data dir, so screenshots and test runs never touch
# the settings of a copy the user is running.
SCRATCH_ENV := XDG_CONFIG_HOME=$(CURDIR)/target/scratch-config XDG_DATA_HOME=$(CURDIR)/target/scratch-data

.DEFAULT_GOAL := help

.PHONY: help run dev build test check screenshot icons install uninstall clean

help: ## Show this help
	@echo "Usage: make <target>"
	@echo
	@awk 'BEGIN { FS = ":.*## " } /^[a-z-]+:.*## / { printf "  \033[1m%-14s\033[0m %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

run: ## Run the app
	cargo run

dev: ## Run with live reload (requires cargo-watch)
	@command -v cargo-watch >/dev/null 2>&1 || { echo "cargo-watch is required: cargo install cargo-watch"; exit 1; }
	cargo watch -x run

build: ## Build a release binary
	cargo build --release

test: ## Run the tests
	cargo test

check: test ## Run the tests and clippy, as before a commit or release
	cargo clippy --all-targets -- -D warnings

screenshot: ## Regenerate docs/screenshot-*.png in a throwaway config dir
	@rm -rf target/scratch-config target/scratch-data
	@mkdir -p docs target/scratch-config target/scratch-data
	$(SCRATCH_ENV) DRAW_SCREENSHOT=docs/screenshot-default.png DRAW_SKIN=Default cargo run
	$(SCRATCH_ENV) DRAW_SCREENSHOT=docs/screenshot-dark-orange.png DRAW_SKIN="Dark Orange" cargo run

icons: ## Render assets/draw-icon-128.png from packaging/linux/draw.svg
	cargo run -q --example render_icon -- 128 assets/draw-icon-128.png

# For the current user; no sudo. The launcher's Exec gets the absolute path,
# so it works even if $(BINDIR) isn't on the session's PATH. On Wayland the
# window icon comes from this launcher, matched by app id.
install: build ## Install Draw and its launcher for the current user (PREFIX=~/.local)
	install -Dm755 target/release/draw $(BINDIR)/draw
	install -Dm644 packaging/linux/draw.svg $(ICONDIR)/scalable/apps/$(APP_ID).svg
	install -Dm644 assets/draw-icon-128.png $(ICONDIR)/128x128/apps/$(APP_ID).png
	install -d $(APPSDIR)
	sed 's|^Exec=draw|Exec=$(BINDIR)/draw|' packaging/linux/$(APP_ID).desktop > $(APPSDIR)/$(APP_ID).desktop
	-update-desktop-database -q $(APPSDIR)
	@# GTK trusts an icon cache over the folder, so refresh one if it's there.
	-@test ! -f $(ICONDIR)/icon-theme.cache || gtk-update-icon-cache -q -t -f $(ICONDIR)
	@echo "Installed $(BINDIR)/draw and the $(APP_ID) launcher"

uninstall: ## Remove what make install put in place
	rm -f $(BINDIR)/draw $(APPSDIR)/$(APP_ID).desktop
	rm -f $(ICONDIR)/scalable/apps/$(APP_ID).svg $(ICONDIR)/128x128/apps/$(APP_ID).png
	-update-desktop-database -q $(APPSDIR)
	-@test ! -f $(ICONDIR)/icon-theme.cache || gtk-update-icon-cache -q -t -f $(ICONDIR)

clean: ## Remove build output
	cargo clean
