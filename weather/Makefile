.DEFAULT_GOAL := help

APP_ID := com.sectorflabs.Weather
PREFIX ?= $(HOME)/.local
BINDIR := $(PREFIX)/bin
APPSDIR := $(PREFIX)/share/applications
ICONDIR := $(PREFIX)/share/icons/hicolor

# Throwaway config, data and cache dirs, so test runs never touch yours.
SCRATCH := target/scratch
SCRATCH_ENV := XDG_CONFIG_HOME=$(CURDIR)/$(SCRATCH)/config XDG_DATA_HOME=$(CURDIR)/$(SCRATCH)/data XDG_CACHE_HOME=$(CURDIR)/$(SCRATCH)/cache

.PHONY: help run dev build test check scratch screenshot icons icon install uninstall clean

help: ## Show this help
	@echo "Usage: make <target>"
	@echo
	@awk 'BEGIN { FS = ":.*## " } /^[a-z-]+:.*## / { printf "  \033[1m%-14s\033[0m %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

run: ## Run Weather with your own settings
	cargo run

dev: scratch ## Run in a scratch config with a few places in it
	$(SCRATCH_ENV) cargo run

build: ## Build a release binary
	cargo build --release

test: ## Run the tests
	cargo test

check: test ## Run the tests and clippy
	cargo clippy --all-targets -- -D warnings

# A scratch config with a few places, for dev runs and screenshots.
scratch:
	@mkdir -p $(SCRATCH)/config/weather
	@test -f $(SCRATCH)/config/weather/config.toml || cp packaging/dev-config.toml $(SCRATCH)/config/weather/config.toml

screenshot: ## Regenerate docs/screenshot-*.png in a scratch config
	@rm -rf $(SCRATCH) && $(MAKE) -s scratch && mkdir -p docs
	$(SCRATCH_ENV) WEATHER_VIEW=overview WEATHER_SCREENSHOT=docs/screenshot-overview.png cargo run
	$(SCRATCH_ENV) WEATHER_VIEW=week WEATHER_SKIN="Dark Orange" WEATHER_SCREENSHOT=docs/screenshot-week.png cargo run
	$(SCRATCH_ENV) WEATHER_VIEW=day WEATHER_DAY=1 WEATHER_SCREENSHOT=docs/screenshot-day.png cargo run

icons: ## Render assets/weather-icon-128.png from packaging/linux/weather.svg
	cargo run -q --example render_icon -- 128 assets/weather-icon-128.png

icon: ## docs/icon.png: the app icon at 120 px, for the README
	scripts/icon.sh

# For the current user; no sudo. The launcher's Exec gets the absolute path,
# so it works even if $(BINDIR) isn't on the GNOME session's PATH. On
# Wayland the window icon comes from this launcher, matched by app id.
install: build ## Install Weather and its launcher for the current user (PREFIX=~/.local)
	install -Dm755 target/release/weather $(BINDIR)/weather
	install -Dm644 packaging/linux/weather.svg $(ICONDIR)/scalable/apps/$(APP_ID).svg
	install -Dm644 assets/weather-icon-128.png $(ICONDIR)/128x128/apps/$(APP_ID).png
	install -d $(APPSDIR)
	sed 's|^Exec=weather|Exec=$(BINDIR)/weather|' packaging/linux/$(APP_ID).desktop > $(APPSDIR)/$(APP_ID).desktop
	-update-desktop-database -q $(APPSDIR)
	@# GTK trusts an icon cache over the folder, so refresh one if it's there.
	-@test ! -f $(ICONDIR)/icon-theme.cache || gtk-update-icon-cache -q -t -f $(ICONDIR)
	@echo "Installed $(BINDIR)/weather and the $(APP_ID) launcher"

uninstall: ## Remove what make install put in place
	rm -f $(BINDIR)/weather $(APPSDIR)/$(APP_ID).desktop
	rm -f $(ICONDIR)/scalable/apps/$(APP_ID).svg $(ICONDIR)/128x128/apps/$(APP_ID).png
	-update-desktop-database -q $(APPSDIR)
	-@test ! -f $(ICONDIR)/icon-theme.cache || gtk-update-icon-cache -q -t -f $(ICONDIR)

clean: ## Remove build output
	cargo clean
