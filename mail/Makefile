.DEFAULT_GOAL := help

APP_ID := com.sectorflabs.Mail
PREFIX ?= $(HOME)/.local
BINDIR := $(PREFIX)/bin
APPSDIR := $(PREFIX)/share/applications
ICONDIR := $(PREFIX)/share/icons/hicolor

# Throwaway config, data and cache dirs, so test runs never touch yours.
SCRATCH := target/scratch
SCRATCH_ENV := XDG_CONFIG_HOME=$(CURDIR)/$(SCRATCH)/config XDG_DATA_HOME=$(CURDIR)/$(SCRATCH)/data XDG_CACHE_HOME=$(CURDIR)/$(SCRATCH)/cache

.PHONY: help run dev demo build test check live-test screenshot icons icon install uninstall clean

help: ## Show this help
	@echo "Usage: make <target>"
	@echo
	@awk 'BEGIN { FS = ":.*## " } /^[a-z-]+:.*## / { printf "  \033[1m%-14s\033[0m %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

run: ## Run Mail with your own settings
	cargo run

dev: ## Run with live reload, in a scratch config (requires cargo-watch)
	@command -v cargo-watch >/dev/null 2>&1 || { echo "cargo-watch is required: cargo install cargo-watch"; exit 1; }
	@mkdir -p $(SCRATCH)/config $(SCRATCH)/data
	$(SCRATCH_ENV) cargo watch -x run

demo: ## Run with made-up mail and no connection, in a scratch config
	@mkdir -p $(SCRATCH)/config $(SCRATCH)/data
	$(SCRATCH_ENV) MAIL_DEMO=1 cargo run

build: ## Build a release binary
	cargo build --release

test: ## Run the tests
	cargo test

check: test ## Run the tests and clippy
	cargo clippy --all-targets -- -D warnings

live-test: ## Run the IMAP tests against your Bridge (MAIL_LIVE_ACCOUNT=path/to/account.toml)
	@test -n "$(MAIL_LIVE_ACCOUNT)" || { echo "set MAIL_LIVE_ACCOUNT to an account.toml"; exit 1; }
	MAIL_LIVE_ACCOUNT=$(MAIL_LIVE_ACCOUNT) cargo test -- --ignored

screenshot: ## Regenerate docs/screenshot-*.png from the demo mail
	@rm -rf $(SCRATCH) && mkdir -p docs $(SCRATCH)/config $(SCRATCH)/data
	$(SCRATCH_ENV) MAIL_DEMO=1 MAIL_SKIN=Default MAIL_SCREENSHOT=docs/screenshot.png cargo run
	$(SCRATCH_ENV) MAIL_DEMO=1 MAIL_SKIN="Dark Orange" MAIL_SCREENSHOT=docs/screenshot-dark-orange.png cargo run
	$(SCRATCH_ENV) MAIL_DEMO=reply MAIL_SKIN=Default MAIL_SCREENSHOT=docs/screenshot-reply.png cargo run
	$(SCRATCH_ENV) MAIL_DEMO=review MAIL_SKIN=Default MAIL_SCREENSHOT=docs/screenshot-images.png cargo run

icons: ## Render assets/mail-icon-128.png from packaging/linux/mail.svg
	cargo run -q --example render_icon -- 128 assets/mail-icon-128.png

icon: ## docs/icon.png: the app icon at 120 px, for the README
	scripts/icon.sh

# For the current user; no sudo. The launcher's Exec gets the absolute path,
# so it works even if $(BINDIR) isn't on the GNOME session's PATH. On
# Wayland the window icon comes from this launcher, matched by app id.
install: build ## Install Mail and its launcher for the current user (PREFIX=~/.local)
	install -Dm755 target/release/mail $(BINDIR)/mail
	install -Dm644 packaging/linux/mail.svg $(ICONDIR)/scalable/apps/$(APP_ID).svg
	install -Dm644 assets/mail-icon-128.png $(ICONDIR)/128x128/apps/$(APP_ID).png
	install -d $(APPSDIR)
	sed 's|^Exec=mail|Exec=$(BINDIR)/mail|' packaging/linux/$(APP_ID).desktop > $(APPSDIR)/$(APP_ID).desktop
	-update-desktop-database -q $(APPSDIR)
	@# GTK trusts an icon cache over the folder, so refresh one if it's there.
	-@test ! -f $(ICONDIR)/icon-theme.cache || gtk-update-icon-cache -q -t -f $(ICONDIR)
	@echo "Installed $(BINDIR)/mail and the $(APP_ID) launcher"

uninstall: ## Remove what make install put in place
	rm -f $(BINDIR)/mail $(APPSDIR)/$(APP_ID).desktop
	rm -f $(ICONDIR)/scalable/apps/$(APP_ID).svg $(ICONDIR)/128x128/apps/$(APP_ID).png
	-update-desktop-database -q $(APPSDIR)
	-@test ! -f $(ICONDIR)/icon-theme.cache || gtk-update-icon-cache -q -t -f $(ICONDIR)

clean: ## Remove build output
	cargo clean
