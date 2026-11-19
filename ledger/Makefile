TARGET := $(CURDIR)/../target
.DEFAULT_GOAL := help

APP_ID := com.sectorflabs.Ledger
PREFIX ?= $(HOME)/.local
BINDIR := $(PREFIX)/bin
APPSDIR := $(PREFIX)/share/applications
ICONDIR := $(PREFIX)/share/icons/hicolor

# A throwaway config and data dir, so screenshots and test runs never touch
# the settings of a copy the user is running. A scratch config hides
# user-dirs.dirs, so copy it in for the file dialogs.
SCRATCH := $(TARGET)/scratch
SCRATCH_ENV := XDG_CONFIG_HOME=$(SCRATCH)/config XDG_DATA_HOME=$(SCRATCH)/data

.PHONY: help run dev build test check scratch screenshot icons icon install uninstall clean

help: ## Show this help
	@echo "Usage: make <target>"
	@echo
	@awk 'BEGIN { FS = ":.*## " } /^[a-z-]+:.*## / { printf "  \033[1m%-14s\033[0m %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

run: ## Run Ledger with your own settings
	cargo run -p ledger

dev: scratch ## Run with the demo books, in a scratch config
	$(SCRATCH_ENV) LEDGER_DEMO=$(SCRATCH)/demo.sqlite3 cargo run -p ledger

build: ## Build a release binary
	cargo build -p ledger --release

test: ## Run the tests
	cargo test -p ledger

check: test ## Run the tests and clippy, as before a commit or release
	cargo clippy -p ledger --all-targets -- -D warnings

scratch:
	@rm -rf $(SCRATCH) && mkdir -p $(SCRATCH)/config $(SCRATCH)/data
	@test ! -f $(HOME)/.config/user-dirs.dirs || cp $(HOME)/.config/user-dirs.dirs $(SCRATCH)/config/

screenshot: scratch ## Regenerate docs/screenshot-*.png from the demo books, in a scratch config
	@mkdir -p docs
	$(SCRATCH_ENV) LEDGER_DEMO=$(SCRATCH)/demo.sqlite3 LEDGER_SCREENSHOT=docs/screenshot-default.png LEDGER_SKIN=Default cargo run -p ledger
	$(SCRATCH_ENV) LEDGER_DEMO=$(SCRATCH)/demo.sqlite3 LEDGER_SCREENSHOT=docs/screenshot-dark-orange.png LEDGER_SKIN="Dark Orange" cargo run -p ledger
	$(SCRATCH_ENV) LEDGER_DEMO=$(SCRATCH)/demo.sqlite3 LEDGER_SCREENSHOT=docs/screenshot-shell.png LEDGER_SHEET=Disk cargo run -p ledger

icons: ## Render assets/ledger-icon-128.png from packaging/linux/ledger.svg
	cargo run -p ledger -q --example render_icon -- 128 assets/ledger-icon-128.png

icon: ## docs/icon.png: the app icon at 120 px, for the README
	scripts/icon.sh

# For the current user; no sudo. The launcher's Exec gets the absolute path,
# so it works even if $(BINDIR) isn't on the GNOME session's PATH. On
# Wayland the window icon comes from this launcher, matched by app id.
install: build ## Install Ledger and its launcher for the current user (PREFIX=~/.local)
	install -Dm755 $(TARGET)/release/ledger $(BINDIR)/ledger
	install -Dm644 packaging/linux/ledger.svg $(ICONDIR)/scalable/apps/$(APP_ID).svg
	install -Dm644 assets/ledger-icon-128.png $(ICONDIR)/128x128/apps/$(APP_ID).png
	install -d $(APPSDIR)
	sed 's|^Exec=ledger|Exec=$(BINDIR)/ledger|' packaging/linux/$(APP_ID).desktop > $(APPSDIR)/$(APP_ID).desktop
	-update-desktop-database -q $(APPSDIR)
	@# GTK trusts an icon cache over the folder, so refresh one if it's there.
	-@test ! -f $(ICONDIR)/icon-theme.cache || gtk-update-icon-cache -q -t -f $(ICONDIR)
	@echo "Installed $(BINDIR)/ledger and the $(APP_ID) launcher"

uninstall: ## Remove what make install put in place
	rm -f $(BINDIR)/ledger $(APPSDIR)/$(APP_ID).desktop
	rm -f $(ICONDIR)/scalable/apps/$(APP_ID).svg $(ICONDIR)/128x128/apps/$(APP_ID).png
	-update-desktop-database -q $(APPSDIR)
	-@test ! -f $(ICONDIR)/icon-theme.cache || gtk-update-icon-cache -q -t -f $(ICONDIR)

clean: ## Remove build output
	cargo clean -p ledger
