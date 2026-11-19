TARGET := $(CURDIR)/../target
.DEFAULT_GOAL := help

APP_ID := com.sectorflabs.Montage
PREFIX ?= $(HOME)/.local
BINDIR := $(PREFIX)/bin
APPSDIR := $(PREFIX)/share/applications
ICONDIR := $(PREFIX)/share/icons/hicolor

# Throwaway config and data dirs, so test runs never touch yours. With no
# Videos folder in them, voiceovers are recorded under the scratch data dir.
SCRATCH := $(TARGET)/scratch
SCRATCH_ENV := XDG_CONFIG_HOME=$(SCRATCH)/config XDG_DATA_HOME=$(SCRATCH)/data
DEMO := $(TARGET)/demo/Demo.montage

.PHONY: help run dev demo build test check screenshot icons icon app install uninstall clean

help: ## Show this help
	@echo "Usage: make <target>"
	@echo
	@awk 'BEGIN { FS = ":.*## " } /^[a-z-]+:.*## / { printf "  \033[1m%-14s\033[0m %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

run: ## Run Montage with your own settings
	cargo run -p montage

dev: ## Run with live reload, in a scratch config (requires cargo-watch)
	@command -v cargo-watch >/dev/null 2>&1 || { echo "cargo-watch is required: cargo install cargo-watch"; exit 1; }
	@mkdir -p $(SCRATCH)/config $(SCRATCH)/data $(SCRATCH)/videos
	$(SCRATCH_ENV) cargo watch -x "run -p montage --bin montage"

$(DEMO): scripts/demo.sh
	scripts/demo.sh

demo: $(DEMO) ## Open a demo project of generated clips, in a scratch config
	@mkdir -p $(SCRATCH)/config $(SCRATCH)/data $(SCRATCH)/videos
	$(SCRATCH_ENV) cargo run -p montage -- $(DEMO)

build: ## Build a release binary
	cargo build -p montage --release

test: ## Run the tests (the export test needs ffmpeg)
	cargo test -p montage

check: test ## Run the tests and clippy, as before a commit
	cargo clippy -p montage --all-targets -- -D warnings

screenshot: $(DEMO) ## Regenerate docs/screenshot-*.png from the demo project
	@rm -rf $(SCRATCH) && mkdir -p docs $(SCRATCH)/config $(SCRATCH)/data $(SCRATCH)/videos
	$(SCRATCH_ENV) MONTAGE_SKIN=Default MONTAGE_SCREENSHOT=docs/screenshot.png cargo run -p montage -- $(DEMO)
	@rm -rf $(SCRATCH) && mkdir -p $(SCRATCH)/config $(SCRATCH)/data $(SCRATCH)/videos
	$(SCRATCH_ENV) MONTAGE_SKIN="Dark Orange" MONTAGE_SCREENSHOT=docs/screenshot-dark-orange.png cargo run -p montage -- $(DEMO)

icons: ## Render assets/montage-icon-128.png from packaging/linux/montage.svg
	cargo run -p montage -q --example render_icon -- 128 assets/montage-icon-128.png

icon: ## docs/icon.png: the app icon at 120 px, for the README
	@mkdir -p docs
	cargo run -p montage -q --example render_icon -- 120 docs/icon.png

app: build ## Make $(TARGET)/Montage.app, a folder app for ~/Apps on the Patina desktop
	rm -rf $(TARGET)/Montage.app && mkdir -p $(TARGET)/Montage.app/bin
	install -m755 $(TARGET)/release/montage $(TARGET)/Montage.app/bin/montage
	install -m644 packaging/linux/montage.svg $(TARGET)/Montage.app/icon.svg
	printf 'format = 1\nname = "Montage"\nexec = "bin/montage"\nid = "$(APP_ID)"\nsummary = "Video Editor"\nkeywords = ["video", "movie", "editor", "film"]\ncategories = ["AudioVideo", "Video"]\nicon = "icon.svg"\n' > $(TARGET)/Montage.app/app.toml
	@echo "Made $(TARGET)/Montage.app"

# For the current user; no sudo. The launcher's Exec gets the absolute path,
# so it works even if $(BINDIR) isn't on the session's PATH. On Wayland the
# window icon comes from this launcher, matched by app id.
install: build ## Install Montage and its launcher for the current user (PREFIX=~/.local)
	install -Dm755 $(TARGET)/release/montage $(BINDIR)/montage
	install -Dm644 packaging/linux/montage.svg $(ICONDIR)/scalable/apps/$(APP_ID).svg
	install -Dm644 assets/montage-icon-128.png $(ICONDIR)/128x128/apps/$(APP_ID).png
	install -d $(APPSDIR)
	sed 's|^Exec=montage|Exec=$(BINDIR)/montage|' packaging/linux/$(APP_ID).desktop > $(APPSDIR)/$(APP_ID).desktop
	-update-desktop-database -q $(APPSDIR)
	@# GTK trusts an icon cache over the folder, so refresh one if it's there.
	-@test ! -f $(ICONDIR)/icon-theme.cache || gtk-update-icon-cache -q -t -f $(ICONDIR)
	@echo "Installed $(BINDIR)/montage and the $(APP_ID) launcher"

uninstall: ## Remove what make install put in place
	rm -f $(BINDIR)/montage $(APPSDIR)/$(APP_ID).desktop
	rm -f $(ICONDIR)/scalable/apps/$(APP_ID).svg $(ICONDIR)/128x128/apps/$(APP_ID).png
	-update-desktop-database -q $(APPSDIR)
	-@test ! -f $(ICONDIR)/icon-theme.cache || gtk-update-icon-cache -q -t -f $(ICONDIR)

clean: ## Remove build output
	cargo clean -p montage
