.DEFAULT_GOAL := help

APP_ID := com.sectorflabs.Notebook
PREFIX ?= $(HOME)/.local
BINDIR := $(PREFIX)/bin
APPSDIR := $(PREFIX)/share/applications
ICONDIR := $(PREFIX)/share/icons/hicolor

# A throwaway config dir, so screenshots never touch your settings.
SCREENSHOT_ENV := XDG_CONFIG_HOME=$(CURDIR)/target/screenshot-config XDG_DATA_HOME=$(CURDIR)/target/screenshot-data

.PHONY: help demo-notes run dev build test check icons install uninstall screenshot clean icon

help: ## Show this help
	@echo "Usage: make <target>"
	@echo
	@awk 'BEGIN { FS = ":.*## " } /^[a-z-]+:.*## / { printf "  \033[1m%-14s\033[0m %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

# The example notes are copied first, so trying things out never edits them.
DEMO := target/demo-notes

demo-notes:
	@rm -rf $(DEMO) && mkdir -p target && cp -r examples/notes $(DEMO)

run: demo-notes ## Run Notebook on a copy of the example notes
	cargo run -- $(DEMO)

dev: demo-notes ## Run with live reload (requires cargo-watch)
	@command -v cargo-watch >/dev/null 2>&1 || { echo "cargo-watch is required: cargo install cargo-watch"; exit 1; }
	cargo watch -x "run -- $(DEMO)"

build: ## Build a release binary
	cargo build --release

test: ## Run the tests
	cargo test

check: test ## Run the tests and clippy
	cargo clippy --all-targets -- -D warnings

icons: ## Render assets/notebook-icon-128.png from packaging/linux/notebook.svg
	cargo run -q --example render_icon -- 128 assets/notebook-icon-128.png

icon: ## docs/icon.png: the app icon at 120 px, for the README
	scripts/icon.sh

# For the current user, as the GTK Notebook did; no sudo. The launcher's
# Exec gets the absolute path, so it works even if $(BINDIR) isn't on the
# GNOME session's PATH.
install: build ## Install Notebook and its launcher for the current user (PREFIX=~/.local)
	install -Dm755 target/release/notebook $(BINDIR)/notebook
	install -Dm644 packaging/linux/notebook.svg $(ICONDIR)/scalable/apps/$(APP_ID).svg
	install -Dm644 assets/notebook-icon-128.png $(ICONDIR)/128x128/apps/$(APP_ID).png
	install -d $(APPSDIR)
	sed 's|^Exec=notebook|Exec=$(BINDIR)/notebook|' packaging/linux/$(APP_ID).desktop > $(APPSDIR)/$(APP_ID).desktop
	-update-desktop-database -q $(APPSDIR)
	@# GTK trusts an icon cache over the folder, so refresh one if it's there.
	-@test ! -f $(ICONDIR)/icon-theme.cache || gtk-update-icon-cache -q -t -f $(ICONDIR)
	@echo "Installed $(BINDIR)/notebook and the $(APP_ID) launcher"

uninstall: ## Remove what make install put in place
	rm -f $(BINDIR)/notebook $(APPSDIR)/$(APP_ID).desktop
	rm -f $(ICONDIR)/scalable/apps/$(APP_ID).svg $(ICONDIR)/128x128/apps/$(APP_ID).png
	-update-desktop-database -q $(APPSDIR)
	-@test ! -f $(ICONDIR)/icon-theme.cache || gtk-update-icon-cache -q -t -f $(ICONDIR)

screenshot: ## Regenerate docs/screenshot-*.png from the example notes
	@rm -rf target/screenshot-config target/screenshot-data
	@mkdir -p docs target/screenshot-config target/screenshot-data
	$(SCREENSHOT_ENV) NOTEBOOK_SCREENSHOT=docs/screenshot-notebook.png cargo run -- examples/notes
	$(SCREENSHOT_ENV) NOTEBOOK_SCREENSHOT=docs/screenshot-light.png NOTEBOOK_SKIN=Default NOTEBOOK_THEME=light cargo run -- examples/notes
	$(SCREENSHOT_ENV) NOTEBOOK_SCREENSHOT=docs/screenshot-dark-orange.png NOTEBOOK_SKIN="Dark Orange" cargo run -- examples/notes

clean: ## Remove build output
	cargo clean
