VERSION := $(shell awk -F\" '/^version = / { print $$2; exit }' Cargo.toml)
# Installed for every user of the machine, as a BSD port is (ADR-0078):
# built as yourself, copied in as root.
PREFIX ?= /usr/local
# Under $(PREFIX)/share/man, man(1) finds the pages through PATH's bin folder.
MANDIR ?= $(PREFIX)/share/man
# Settings' launcher entry and icon (ADR-0062), and the session's default
# apps (ADR-0066).
APPSDIR := $(PREFIX)/share/applications
ICONDIR := $(PREFIX)/share/icons/hicolor
SETTINGS_ID := com.sectorflabs.Settings
PROCESSES_ID := com.sectorflabs.ProcessManager
# The login screen's session entry and the lock screen's PAM service go
# where the system looks: on FreeBSD under PREFIX, as ports put them; on
# Linux, where every login manager reads sessions, and /etc/pam.d.
ifeq ($(shell uname -s),FreeBSD)
SESSIONDIR ?= $(PREFIX)/share/wayland-sessions
PAMDIR ?= $(PREFIX)/etc/pam.d
else
SESSIONDIR ?= /usr/share/wayland-sessions
PAMDIR ?= /etc/pam.d
endif
# How install becomes root when run as yourself: doas, else sudo.
AS_ROOT := $(shell command -v doas >/dev/null && echo doas || echo sudo)
ROOT := $(filter 0,$(shell id -u))

# A session bus of its own for the nested compositor, so it and what it
# starts (bar, tiler, tray) never take the names of a Patina session the
# window opens on.
OWN_BUS := $(shell command -v dbus-run-session >/dev/null && echo "dbus-run-session --")

# grim inside the VM's dwl session, which bsdt runs headless.
VM_WAYLAND := XDG_RUNTIME_DIR=/tmp/bsdt-xdg WAYLAND_DISPLAY=wayland-0

.DEFAULT_GOAL := help

.PHONY: help deps run run-bar build test check install stage deploy uninstall vm vm-restart vm-gui screenshot vm-linux vm-linux-restart vm-linux-shot vm-linux-gui vm-linux-stop clean

help: ## Show this help
	@echo "Usage: make <target>"
	@echo
	@awk 'BEGIN { FS = ":.*## " } /^[a-z-]+:.*## / { printf "  \033[1m%-17s\033[0m %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

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

build: ## Build the release binaries, and the launcher from its repository
	cargo build --release
	cargo install --quiet --locked --git $(LAUNCHER) --root target/launcher patina-launcher

test: ## Run the tests
	cargo test --workspace

check: test ## Run the tests and clippy, and lint the man pages, as before a commit or release
	cargo clippy --workspace --all-targets -- -D warnings
	@if command -v mandoc >/dev/null; then mandoc -Tlint -Wwarning man/*; \
	else echo "mandoc isn't installed; the man pages weren't linted"; fi

BINARIES := patina-bar patina-clock patina-compositor patina-dictate patina-lock patina-menu patina-night-light patina-power patina-processes patina-raise patina-session patina-settings patina-tiler patina-volume patina-wallpaper patina-wifi
# The launcher lives in its own repository (ADR-0037); install builds it from there.
LAUNCHER := https://github.com/Sector-F-Labs/patina-launcher
# The session's terminal, which deploy installs too: the checkout beside this
# one, as it is, else GitHub's.
TELEPRINTER ?= $(abspath ../teleprinter)
TELEPRINTER_REPO := https://github.com/Sector-F-Labs/teleprinter

ifeq ($(ROOT),)
install: build ## Build, then install for every user into /usr/local (PREFIX), with the login session (asks doas or sudo)
	$(AS_ROOT) $(MAKE) --no-print-directory install PREFIX=$(PREFIX) MANDIR=$(MANDIR) SESSIONDIR=$(SESSIONDIR) PAMDIR=$(PAMDIR)
else
# As root it never builds, so nothing in target/ ends up root's.
install:
	@test -x target/release/patina-compositor -a -x target/launcher/bin/patina-launcher || \
		{ echo "Nothing built: run make build first, as yourself."; exit 1; }
	$(MAKE) --no-print-directory stage
	-update-desktop-database -q $(APPSDIR)
	@# GTK trusts an icon cache over the folder, so refresh one if it exists.
	-@test ! -f $(ICONDIR)/icon-theme.cache || gtk-update-icon-cache -q -t -f $(ICONDIR)
	@echo "Installed into $(PREFIX), with $(SESSIONDIR)/patina.desktop and $(PAMDIR)/patina-lock."
endif

# What install puts in place, under $(DESTDIR) if set: deploy stages it
# there for another machine. Built already.
stage:
	install -d $(DESTDIR)$(PREFIX)/bin $(DESTDIR)$(MANDIR)/man1 $(DESTDIR)$(MANDIR)/man7
	install -m 755 $(addprefix target/release/,$(BINARIES)) target/launcher/bin/patina-launcher $(DESTDIR)$(PREFIX)/bin/
	install -m 644 man/*.1 $(DESTDIR)$(MANDIR)/man1/
	install -m 644 man/*.7 $(DESTDIR)$(MANDIR)/man7/
	@# The entries' Exec gets the absolute path, so it works whatever PATH is.
	install -d $(DESTDIR)$(ICONDIR)/scalable/apps $(DESTDIR)$(APPSDIR) $(DESTDIR)$(SESSIONDIR) $(DESTDIR)$(PAMDIR)
	install -m 644 settings/packaging/settings.svg $(DESTDIR)$(ICONDIR)/scalable/apps/$(SETTINGS_ID).svg
	sed 's|^Exec=patina-settings|Exec=$(PREFIX)/bin/patina-settings|' settings/packaging/$(SETTINGS_ID).desktop > $(DESTDIR)$(APPSDIR)/$(SETTINGS_ID).desktop
	install -m 644 processes/packaging/processes.svg $(DESTDIR)$(ICONDIR)/scalable/apps/$(PROCESSES_ID).svg
	sed 's|^Exec=patina-processes|Exec=$(PREFIX)/bin/patina-processes|' processes/packaging/$(PROCESSES_ID).desktop > $(DESTDIR)$(APPSDIR)/$(PROCESSES_ID).desktop
	install -m 644 packaging/patina-mimeapps.list $(DESTDIR)$(APPSDIR)/
	sed 's|@BINDIR@|$(PREFIX)/bin|' packaging/linux/patina.desktop > $(DESTDIR)$(SESSIONDIR)/patina.desktop
	install -m 644 packaging/linux/patina-lock.pam $(DESTDIR)$(PAMDIR)/patina-lock

# `make deploy nifl` (or HOST=nifl): the host is the goal after deploy.
ifeq (deploy,$(firstword $(MAKECMDGOALS)))
HOST ?= $(word 2,$(MAKECMDGOALS))
ifneq ($(HOST),)
$(eval $(HOST):;@:)
endif
endif
DEPLOY_DIR := target/deploy

deploy: build ## Build here and install on another machine over ssh, Teleprinter too: make deploy <host> (doas there)
	@test -n "$(HOST)" || { echo "usage: make deploy <host>, an ssh host with rsync and doas"; exit 1; }
	rm -rf $(DEPLOY_DIR)
	$(MAKE) --no-print-directory stage DESTDIR=$(CURDIR)/$(DEPLOY_DIR)
	src="$(TELEPRINTER)" && { test -f "$$src/Makefile" || { src=$(CURDIR)/target/teleprinter && \
		rm -rf "$$src" && git clone -q --depth 1 $(TELEPRINTER_REPO) "$$src"; }; } && \
	$(MAKE) --no-print-directory -C "$$src" install DESTDIR=$(CURDIR)/$(DEPLOY_DIR) PREFIX=$(PREFIX)
	@# As root there, owned by root. Without -p, what's there already keeps
	@# its permissions (with -p, / and /etc took the staged folders'); new
	@# files get theirs less root's umask, and -E keeps what's executable.
	rsync -rltE --no-owner --no-group --omit-dir-times --rsync-path="doas rsync" \
		$(DEPLOY_DIR)/ $(HOST):/
	@# Copied files replace the old by rename, so what's running keeps going.
	-ssh $(HOST) 'doas update-desktop-database -q $(APPSDIR); \
		test ! -f $(ICONDIR)/icon-theme.cache || doas gtk-update-icon-cache -q -t -f $(ICONDIR)'
	@echo "Installed on $(HOST). Restart the desktop there to use it: Ctrl+Alt+Shift+Backspace."

ifeq ($(ROOT),)
uninstall: ## Remove what install put in place (asks doas or sudo)
	$(AS_ROOT) $(MAKE) --no-print-directory uninstall PREFIX=$(PREFIX) MANDIR=$(MANDIR) SESSIONDIR=$(SESSIONDIR) PAMDIR=$(PAMDIR)
else
uninstall:
	rm -f $(addprefix $(PREFIX)/bin/,$(BINARIES) patina-launcher)
	rm -f $(addprefix $(MANDIR)/man1/,$(notdir $(wildcard man/*.1)))
	rm -f $(addprefix $(MANDIR)/man7/,$(notdir $(wildcard man/*.7)))
	rm -f $(APPSDIR)/$(SETTINGS_ID).desktop $(APPSDIR)/patina-mimeapps.list $(ICONDIR)/scalable/apps/$(SETTINGS_ID).svg \
		$(APPSDIR)/$(PROCESSES_ID).desktop $(ICONDIR)/scalable/apps/$(PROCESSES_ID).svg
	rm -f $(SESSIONDIR)/patina.desktop $(PAMDIR)/patina-lock
	-update-desktop-database -q $(APPSDIR)
endif

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

vm-linux: build ## Boot the Linux test VM: a Patina session on tty1, on the binaries built here (ADR-0017)
	vm-linux/vm.sh start

vm-linux-restart: build ## Rebuild here and restart the Linux VM's session
	vm-linux/vm.sh restart

vm-linux-shot: ## Save the Linux VM's screen to target/vm-linux/screen.png
	vm-linux/vm.sh shot

vm-linux-gui: ## Open the Linux VM's screen in a VNC viewer
	vm-linux/vm.sh view

vm-linux-stop: ## Power the Linux VM off (vm-linux/vm.sh reset deletes its disk)
	vm-linux/vm.sh stop

clean: ## Remove build output
	cargo clean
