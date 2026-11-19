.DEFAULT_GOAL := help

APP_ID := com.sectorflabs.Calendar
PREFIX ?= $(HOME)/.local
BINDIR := $(PREFIX)/bin
APPSDIR := $(PREFIX)/share/applications
ICONDIR := $(PREFIX)/share/icons/hicolor

# A local plan server with made-up calendars, for development and tests.
PLAN ?= ../../plan/target/release/plan
TARGET := $(CURDIR)/../target
DEV := $(TARGET)/dev-server
DEV_PORT := 5299
DEV_URL := http://127.0.0.1:$(DEV_PORT)/

# Throwaway config, data and cache dirs, so test runs never touch yours.
SCRATCH := $(TARGET)/scratch
SCRATCH_ENV := XDG_CONFIG_HOME=$(SCRATCH)/config XDG_DATA_HOME=$(SCRATCH)/data XDG_CACHE_HOME=$(SCRATCH)/cache

.PHONY: help run dev build test check live-test dev-server dev-server-stop scratch screenshot icons install uninstall clean icon

help: ## Show this help
	@echo "Usage: make <target>"
	@echo
	@awk 'BEGIN { FS = ":.*## " } /^[a-z-]+:.*## / { printf "  \033[1m%-14s\033[0m %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

run: ## Run Calendar with your own settings
	cargo run -p calendar

dev: scratch dev-server ## Run against the local dev server, in a scratch config
	$(SCRATCH_ENV) cargo run -p calendar

build: ## Build a release binary
	cargo build --release -p calendar

test: ## Run the tests
	cargo test -p calendar

check: test ## Run the tests and clippy
	cargo clippy -p calendar --all-targets -- -D warnings

live-test: dev-server ## Run the CalDAV tests against the local dev server
	CALENDAR_DEV_SERVER=$(DEV_URL) cargo test -p calendar -- --ignored

dev-server: ## Start a local plan server with seeded calendars (needs ../plan built)
	@test -x $(PLAN) || { echo "build plan first: make -C ../../plan build"; exit 1; }
	@if curl -s -o /dev/null $(DEV_URL); then echo "dev server already running at $(DEV_URL)"; else \
		rm -rf $(DEV) && mkdir -p $(DEV)/config && python3 scripts/seed-dev-calendars.py $(DEV)/data && \
		(XDG_DATA_HOME=$(DEV)/data XDG_CONFIG_HOME=$(DEV)/config \
			nohup $(PLAN) serve --port $(DEV_PORT) --username test --password test > $(DEV)/server.log 2>&1 &) && \
		sleep 1 && echo "dev server at $(DEV_URL) (user test, password test)"; fi

dev-server-stop: ## Stop the local plan server
	-pkill -f "plan serve --port $(DEV_PORT)"

# A scratch config whose account points at the dev server.
scratch:
	@mkdir -p $(SCRATCH)/config/calendar
	@printf 'url = "$(DEV_URL)"\nusername = "test"\npassword = "test"\n' > $(SCRATCH)/config/calendar/account.toml

screenshot: dev-server ## Regenerate docs/screenshot-*.png against the dev server
	@rm -rf $(SCRATCH) && $(MAKE) -s scratch && mkdir -p docs
	$(SCRATCH_ENV) CALENDAR_VIEW=week CALENDAR_SCREENSHOT=docs/screenshot-week.png cargo run -p calendar
	@rm -rf $(SCRATCH) && $(MAKE) -s scratch
	$(SCRATCH_ENV) CALENDAR_VIEW=month CALENDAR_SKIN="Dark Orange" CALENDAR_SCREENSHOT=docs/screenshot-month.png cargo run -p calendar
	@rm -rf $(SCRATCH) && $(MAKE) -s scratch
	$(SCRATCH_ENV) CALENDAR_VIEW=day CALENDAR_SCREENSHOT=docs/screenshot-day.png cargo run -p calendar

icons: ## Render assets/calendar-icon-128.png from packaging/linux/calendar.svg
	cargo run -q -p calendar --example render_icon -- 128 assets/calendar-icon-128.png

icon: ## docs/icon.png: the app icon at 120 px, for the README
	scripts/icon.sh

# For the current user; no sudo. The launcher's Exec gets the absolute path,
# so it works even if $(BINDIR) isn't on the GNOME session's PATH. On
# Wayland the window icon comes from this launcher, matched by app id.
install: build ## Install Calendar and its launcher for the current user (PREFIX=~/.local)
	install -Dm755 $(TARGET)/release/calendar $(BINDIR)/calendar
	install -Dm644 packaging/linux/calendar.svg $(ICONDIR)/scalable/apps/$(APP_ID).svg
	install -Dm644 assets/calendar-icon-128.png $(ICONDIR)/128x128/apps/$(APP_ID).png
	install -d $(APPSDIR)
	sed 's|^Exec=calendar|Exec=$(BINDIR)/calendar|' packaging/linux/$(APP_ID).desktop > $(APPSDIR)/$(APP_ID).desktop
	-update-desktop-database -q $(APPSDIR)
	@# GTK trusts an icon cache over the folder, so refresh one if it's there.
	-@test ! -f $(ICONDIR)/icon-theme.cache || gtk-update-icon-cache -q -t -f $(ICONDIR)
	@echo "Installed $(BINDIR)/calendar and the $(APP_ID) launcher"

uninstall: ## Remove what make install put in place
	rm -f $(BINDIR)/calendar $(APPSDIR)/$(APP_ID).desktop
	rm -f $(ICONDIR)/scalable/apps/$(APP_ID).svg $(ICONDIR)/128x128/apps/$(APP_ID).png
	-update-desktop-database -q $(APPSDIR)
	-@test ! -f $(ICONDIR)/icon-theme.cache || gtk-update-icon-cache -q -t -f $(ICONDIR)

clean: ## Remove build output
	cargo clean -p calendar
