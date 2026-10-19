VERSION := $(shell awk -F\" '/^version = / { print $$2; exit }' Cargo.toml)
PREFIX ?= $(HOME)/.local
# Under $(PREFIX)/share/man, man(1) finds the pages through PATH's bin folder.
MANDIR ?= $(PREFIX)/share/man
# Settings' launcher entry and icon (ADR-0062), and the session's default
# apps (ADR-0066).
APPSDIR := $(PREFIX)/share/applications
ICONDIR := $(PREFIX)/share/icons/hicolor
SETTINGS_ID := com.sectorflabs.Settings

# A session bus of its own for the nested compositor, so it and what it
# starts (bar, tiler, tray) never take the names of a Patina session the
# window opens on.
OWN_BUS := $(shell command -v dbus-run-session >/dev/null && echo "dbus-run-session --")

# grim inside the VM's dwl session, which bsdt runs headless.
VM_WAYLAND := XDG_RUNTIME_DIR=/tmp/bsdt-xdg WAYLAND_DISPLAY=wayland-0

.DEFAULT_GOAL := help

.PHONY: help deps run run-bar build test check install stage deploy uninstall install-session uninstall-session vm vm-restart vm-gui screenshot clean

help: ## Show this help
	@echo "Usage: make <target>"
	@echo
	@awk 'BEGIN { FS = ":.*## " } /^[a-z-]+:.*## / { printf "  \033[1m%-14s\033[0m %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

deps: ## Show the system packages the build needs
	@echo "Ubuntu/Debian: sudo apt install pkg-config libxkbcommon-dev libpixman-1-dev libudev-dev \\"
	@echo "                   libinput-dev libseat-dev libgbm-dev libegl-dev libdrm-dev libwayland-dev \\"
	@echo "                   libasound2-dev cmake clang glslc libvulkan-dev"
	@echo "FreeBSD:       pkg install pkgconf libxkbcommon pixman libudev-devd libinput seatd mesa-libs libdrm wayland cmake llvm \\"
	@echo "                   shaderc vulkan-headers vulkan-loader"
	@echo "To run X11 apps (Steam), Xwayland too: apt install xwayland, or pkg install xwayland."

run: ## Run the compositor in a window on this desktop, with the bar, on a bus of its own
	cargo build --workspace
	$(OWN_BUS) target/debug/patina-compositor

run-bar: ## Run only the bar on the current compositor (needs layer-shell; not GNOME)
	cargo run -p patina-bar < /dev/null

build: ## Build a release binary
	cargo build --release

test: ## Run the tests
	cargo test --workspace

check: test ## Run the tests and clippy, and lint the man pages, as before a commit or release
	cargo clippy --workspace --all-targets -- -D warnings
	@if command -v mandoc >/dev/null; then mandoc -Tlint -Wwarning man/*; \
	else echo "mandoc isn't installed; the man pages weren't linted"; fi

BINARIES := patina-bar patina-compositor patina-dictate patina-lock patina-menu patina-night-light patina-power patina-raise patina-session patina-settings patina-tiler patina-volume patina-wallpaper patina-wifi
# The launcher lives in its own repository (ADR-0037); install builds it from there.
LAUNCHER := https://github.com/Sector-F-Labs/patina-launcher
# The session's terminal, which deploy installs too: the checkout beside this
# one, as it is, else GitHub's.
TELEPRINTER ?= $(abspath ../teleprinter)
TELEPRINTER_REPO := https://github.com/Sector-F-Labs/teleprinter

install: stage ## Install the binaries into $(PREFIX)/bin, their man pages into $(MANDIR), Settings' launcher entry and the session's default apps
	-update-desktop-database -q $(APPSDIR)
	@# GTK trusts an icon cache over the folder, so refresh one if it exists.
	-@test ! -f $(ICONDIR)/icon-theme.cache || gtk-update-icon-cache -q -t -f $(ICONDIR)

# What install puts in place, under $(DESTDIR) if set: deploy stages it
# there for another machine, with that machine's PREFIX.
stage: build
	install -d $(DESTDIR)$(PREFIX)/bin $(DESTDIR)$(MANDIR)/man1 $(DESTDIR)$(MANDIR)/man7
	install -m 755 $(addprefix target/release/,$(BINARIES)) $(DESTDIR)$(PREFIX)/bin/
	install -m 644 man/*.1 $(DESTDIR)$(MANDIR)/man1/
	install -m 644 man/*.7 $(DESTDIR)$(MANDIR)/man7/
	@# The entry's Exec gets the absolute path, so it works whatever PATH is.
	install -Dm644 settings/packaging/settings.svg $(DESTDIR)$(ICONDIR)/scalable/apps/$(SETTINGS_ID).svg
	install -d $(DESTDIR)$(APPSDIR)
	sed 's|^Exec=patina-settings|Exec=$(PREFIX)/bin/patina-settings|' settings/packaging/$(SETTINGS_ID).desktop > $(DESTDIR)$(APPSDIR)/$(SETTINGS_ID).desktop
	install -m 644 packaging/patina-mimeapps.list $(DESTDIR)$(APPSDIR)/
	cargo install --quiet --locked --force --git $(LAUNCHER) --root $(DESTDIR)$(PREFIX) patina-launcher

# `make deploy nifl` (or HOST=nifl): the host is the goal after deploy.
ifeq (deploy,$(firstword $(MAKECMDGOALS)))
HOST ?= $(word 2,$(MAKECMDGOALS))
ifneq ($(HOST),)
$(eval $(HOST):;@:)
endif
endif
DEPLOY_DIR := target/deploy

deploy: build ## Build here and install on another machine over ssh, Teleprinter too: make deploy <host>
	@test -n "$(HOST)" || { echo "usage: make deploy <host>, an ssh host with rsync"; exit 1; }
	rm -rf $(DEPLOY_DIR)
	@# Its PREFIX in its home, which may not be where this one's is.
	remote=$$(ssh $(HOST) 'printf %s "$$HOME"') && test -n "$$remote" && \
	$(MAKE) --no-print-directory stage DESTDIR=$(CURDIR)/$(DEPLOY_DIR) PREFIX="$$remote/.local" && \
	src="$(TELEPRINTER)" && { test -f "$$src/Makefile" || { src=$(CURDIR)/target/teleprinter && \
		rm -rf "$$src" && git clone -q --depth 1 $(TELEPRINTER_REPO) "$$src"; }; } && \
	$(MAKE) --no-print-directory -C "$$src" install DESTDIR=$(CURDIR)/$(DEPLOY_DIR) PREFIX="$$remote/.local" && \
	rsync -a --exclude='.crates*' $(DEPLOY_DIR)$$remote/ $(HOST):$$remote/
	@# Copied files replace the old by rename, so what's running keeps going.
	-ssh $(HOST) 'update-desktop-database -q ~/.local/share/applications; \
		test ! -f ~/.local/share/icons/hicolor/icon-theme.cache || gtk-update-icon-cache -q -t -f ~/.local/share/icons/hicolor'
	@echo "Installed on $(HOST). Restart the desktop there to use it: Ctrl+Alt+Shift+Backspace."

uninstall: ## Remove the installed binaries and man pages
	rm -f $(addprefix $(PREFIX)/bin/,$(BINARIES))
	rm -f $(addprefix $(MANDIR)/man1/,$(notdir $(wildcard man/*.1)))
	rm -f $(addprefix $(MANDIR)/man7/,$(notdir $(wildcard man/*.7)))
	rm -f $(APPSDIR)/$(SETTINGS_ID).desktop $(APPSDIR)/patina-mimeapps.list $(ICONDIR)/scalable/apps/$(SETTINGS_ID).svg
	-update-desktop-database -q $(APPSDIR)
	-cargo uninstall --root $(PREFIX) patina-launcher

# Login sessions and PAM services are only read from system folders, so
# these two files need root. The binaries stay per user; run these without
# sudo, or PREFIX would be root's home.
SESSION_DIR := /usr/share/wayland-sessions
PAM_DIR := /etc/pam.d

install-session: install ## Also add "Patina" to the login screen's sessions, and the lock screen's PAM service (asks for sudo)
	@test "$$(id -u)" != 0 || { echo "Run make install-session without sudo; it asks for sudo itself."; exit 1; }
	@mkdir -p target
	sed 's|@BINDIR@|$(PREFIX)/bin|' packaging/linux/patina.desktop > target/patina.desktop
	sudo install -D -m 644 target/patina.desktop $(SESSION_DIR)/patina.desktop
	sudo install -D -m 644 packaging/linux/patina-lock.pam $(PAM_DIR)/patina-lock
	@echo "Installed $(SESSION_DIR)/patina.desktop and $(PAM_DIR)/patina-lock."

uninstall-session: ## Remove "Patina" from the login screen's sessions, and the PAM service (asks for sudo)
	@test "$$(id -u)" != 0 || { echo "Run make uninstall-session without sudo; it asks for sudo itself."; exit 1; }
	sudo rm -f $(SESSION_DIR)/patina.desktop $(PAM_DIR)/patina-lock

vm: ## Boot the FreeBSD test VM: the compositor in a window on dwl (bsdt up)
	bsdt up

vm-restart: ## Rebuild in the VM and restart its desktop
	bsdt exec -- cargo build --release
	bsdt gui --restart

vm-gui: ## Open the VM's desktop in a VNC viewer
	bsdt gui

screenshot: ## Save the VM's desktop to docs/screenshot.png
	@mkdir -p docs
	bsdt exec -- sh -c '$(VM_WAYLAND) grim /tmp/patina-desktop.png'
	bsdt exec -- cat /tmp/patina-desktop.png > docs/screenshot.png

clean: ## Remove build output
	cargo clean
