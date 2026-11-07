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
# Files (ADR-0106): its launcher entry and icon, and the file chooser it is
# for the session, as a portal backend.
FILES_ID := com.sectorflabs.Files
PORTAL_SERVICE := org.freedesktop.impl.portal.desktop.patina
# Reader and Notebook (ADR-0107): launcher entries and icons.
READER_ID := com.sectorflabs.Reader
NOTEBOOK_ID := com.sectorflabs.Notebook
# The login screen's session entry and the lock screen's PAM service go
# where the system looks: on FreeBSD under PREFIX, as ports put them; on
# Linux, where every login manager reads sessions, and /etc/pam.d.
ifeq ($(shell uname -s),FreeBSD)
SESSIONDIR ?= $(PREFIX)/share/wayland-sessions
PAMDIR ?= $(PREFIX)/etc/pam.d
XDGCONFDIR ?= $(PREFIX)/etc/xdg
PORTALDIR ?= $(PREFIX)/share/xdg-desktop-portal
else
SESSIONDIR ?= /usr/share/wayland-sessions
PAMDIR ?= /etc/pam.d
XDGCONFDIR ?= /etc/xdg
# xdg-desktop-portal reads .portal files only from the system's folders.
PORTALDIR ?= /usr/share/xdg-desktop-portal
endif
# Screen sharing through xdg-desktop-portal-wlr (ADR-0092): which portal
# backends the session uses, and the portal's chooser.
PORTALCONF := $(XDGCONFDIR)/xdg-desktop-portal/patina-portals.conf
WLRCONF := $(XDGCONFDIR)/xdg-desktop-portal-wlr/Patina
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

.PHONY: help deps run run-hud build test check install stage deploy deb install-full uninstall-full uninstall vm vm-restart vm-gui screenshot vm-linux vm-linux-restart vm-linux-shot vm-linux-screenshot vm-linux-gui test-vm test-vm-rc vm-linux-stop clean

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
	@echo "To build the .deb (make deb): dpkg-dev, and Rust from rustup.rs."
	@echo "To run X11 apps (Steam), Xwayland too: apt install xwayland, or pkg install xwayland."
	@echo "To record the screen, and for apps to share it: apt install ffmpeg xdg-desktop-portal-wlr,"
	@echo "or pkg install ffmpeg xdg-desktop-portal-wlr."

run: ## Run the compositor in a window on this desktop, with the bar, on a bus of its own
	cargo build --workspace
	$(OWN_BUS) target/debug/patina-compositor

run-hud: ## Run only the HUD on the current compositor (needs layer-shell; not GNOME)
	cargo run -p patina-hud < /dev/null

build: ## Build the release binaries
	cargo build --release

test: ## Run the tests
	cargo test --workspace

check: test ## Run the tests and clippy, and lint the man pages, as before a commit or release
	cargo clippy --workspace --all-targets -- -D warnings
	@if command -v mandoc >/dev/null; then mandoc -Tlint -Wwarning man/*; \
	else echo "mandoc isn't installed; the man pages weren't linted"; fi

BINARIES := files notebook patina-hud patina-clipboard patina-clock patina-compositor patina-dictate patina-launcher patina-lock patina-menu patina-night-light patina-power patina-processes patina-raise reader patina-screenshot patina-session patina-settings patina-tiler patina-volume patina-wallpaper patina-wifi
# The session's terminal, which deploy installs too: the checkout beside this
# one, as it is, else GitHub's.
TELEPRINTER ?= $(abspath ../teleprinter)
TELEPRINTER_REPO := https://github.com/Sector-F-Labs/teleprinter

ifeq ($(ROOT),)
install: build ## Build, then install this repository for every user into /usr/local (PREFIX), with the login session; the apps from other repositories are install-full (asks doas or sudo)
	$(AS_ROOT) $(MAKE) --no-print-directory install PREFIX=$(PREFIX) MANDIR=$(MANDIR) SESSIONDIR=$(SESSIONDIR) PAMDIR=$(PAMDIR) XDGCONFDIR=$(XDGCONFDIR)
else
# As root it never builds, so nothing in target/ ends up root's.
install:
	@test -x target/release/patina-compositor || \
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
	install -m 755 $(addprefix target/release/,$(BINARIES)) $(DESTDIR)$(PREFIX)/bin/
	install -m 644 man/*.1 $(DESTDIR)$(MANDIR)/man1/
	install -m 644 man/*.7 $(DESTDIR)$(MANDIR)/man7/
	@# The entries' Exec gets the absolute path, so it works whatever PATH is.
	install -d $(DESTDIR)$(ICONDIR)/scalable/apps $(DESTDIR)$(APPSDIR) $(DESTDIR)$(SESSIONDIR) $(DESTDIR)$(PAMDIR)
	install -m 644 settings/packaging/settings.svg $(DESTDIR)$(ICONDIR)/scalable/apps/$(SETTINGS_ID).svg
	sed 's|^Exec=patina-settings|Exec=$(PREFIX)/bin/patina-settings|' settings/packaging/$(SETTINGS_ID).desktop > $(DESTDIR)$(APPSDIR)/$(SETTINGS_ID).desktop
	install -m 644 processes/packaging/processes.svg $(DESTDIR)$(ICONDIR)/scalable/apps/$(PROCESSES_ID).svg
	sed 's|^Exec=patina-processes|Exec=$(PREFIX)/bin/patina-processes|' processes/packaging/$(PROCESSES_ID).desktop > $(DESTDIR)$(APPSDIR)/$(PROCESSES_ID).desktop
	install -d $(DESTDIR)$(PORTALDIR)/portals $(DESTDIR)$(PREFIX)/share/dbus-1/services
	install -m 644 files/packaging/linux/files.svg $(DESTDIR)$(ICONDIR)/scalable/apps/$(FILES_ID).svg
	sed 's|^Exec=files|Exec=$(PREFIX)/bin/files|' files/packaging/linux/$(FILES_ID).desktop > $(DESTDIR)$(APPSDIR)/$(FILES_ID).desktop
	install -m 644 files/packaging/portal/patina.portal $(DESTDIR)$(PORTALDIR)/portals/patina.portal
	sed 's|@BINDIR@|$(PREFIX)/bin|' files/packaging/portal/$(PORTAL_SERVICE).service > $(DESTDIR)$(PREFIX)/share/dbus-1/services/$(PORTAL_SERVICE).service
	install -m 644 reader/packaging/linux/reader.svg $(DESTDIR)$(ICONDIR)/scalable/apps/$(READER_ID).svg
	sed 's|^Exec=reader|Exec=$(PREFIX)/bin/reader|' reader/packaging/linux/$(READER_ID).desktop > $(DESTDIR)$(APPSDIR)/$(READER_ID).desktop
	install -d $(DESTDIR)$(ICONDIR)/128x128/apps
	install -m 644 notebook/packaging/linux/notebook.svg $(DESTDIR)$(ICONDIR)/scalable/apps/$(NOTEBOOK_ID).svg
	install -m 644 notebook/assets/notebook-icon-128.png $(DESTDIR)$(ICONDIR)/128x128/apps/$(NOTEBOOK_ID).png
	sed 's|^Exec=notebook|Exec=$(PREFIX)/bin/notebook|' notebook/packaging/linux/$(NOTEBOOK_ID).desktop > $(DESTDIR)$(APPSDIR)/$(NOTEBOOK_ID).desktop
	install -m 644 packaging/patina-mimeapps.list $(DESTDIR)$(APPSDIR)/
	sed 's|@BINDIR@|$(PREFIX)/bin|' packaging/linux/patina.desktop > $(DESTDIR)$(SESSIONDIR)/patina.desktop
	install -m 644 packaging/linux/patina-lock.pam $(DESTDIR)$(PAMDIR)/patina-lock
	install -d $(dir $(DESTDIR)$(PORTALCONF)) $(dir $(DESTDIR)$(WLRCONF))
	install -m 644 packaging/portal/patina-portals.conf $(DESTDIR)$(PORTALCONF)
	sed 's|@BINDIR@|$(PREFIX)/bin|' packaging/portal/xdg-desktop-portal-wlr.ini > $(DESTDIR)$(WLRCONF)

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

deb: ## Build the .deb: the desktop and the apps packaging/bundle.toml pins, into target/deb (WORKTREE=1 for uncommitted changes)
	packaging/deb/build.py $(if $(WORKTREE),--worktree)

ifeq ($(ROOT),)
install-full: ## Install this repository and the apps from other repositories bundle.toml pins, built from GitHub, into /usr/local (PREFIX) (any OS; asks doas or sudo)
	packaging/deb/build.py --stage target/full --prefix $(PREFIX)
	$(AS_ROOT) $(MAKE) --no-print-directory install-full PREFIX=$(PREFIX)
else
install-full:
	@test -d target/full || { echo "Nothing staged: run make install-full as yourself."; exit 1; }
	cp -R target/full/. $(DESTDIR)/
	-update-desktop-database -q $(DESTDIR)$(APPSDIR)
	-@test ! -f $(DESTDIR)$(ICONDIR)/icon-theme.cache || gtk-update-icon-cache -q -t -f $(DESTDIR)$(ICONDIR)
	@echo "Installed the whole desktop into $(PREFIX): what $(PREFIX)/share/patina-desktop/versions lists."
endif

ifeq ($(ROOT),)
uninstall-full: ## Remove what install-full put in place, every file it listed (asks doas or sudo)
	$(AS_ROOT) $(MAKE) --no-print-directory uninstall-full PREFIX=$(PREFIX)
else
uninstall-full:
	@test -f $(DESTDIR)$(PREFIX)/share/patina-desktop/files || { echo "Nothing to remove: install-full didn't leave $(PREFIX)/share/patina-desktop/files."; exit 1; }
	@while IFS= read -r f; do rm -f "$(DESTDIR)$$f"; done < $(DESTDIR)$(PREFIX)/share/patina-desktop/files
	-@find $(DESTDIR)$(PREFIX)/share/doc/patina-desktop $(DESTDIR)$(PREFIX)/share/patina-desktop -depth -type d -empty -delete 2>/dev/null
	-update-desktop-database -q $(DESTDIR)$(APPSDIR)
	@echo "Removed the desktop and its apps from $(PREFIX)."
endif

ifeq ($(ROOT),)
uninstall: ## Remove what install put in place (asks doas or sudo)
	$(AS_ROOT) $(MAKE) --no-print-directory uninstall PREFIX=$(PREFIX) MANDIR=$(MANDIR) SESSIONDIR=$(SESSIONDIR) PAMDIR=$(PAMDIR) XDGCONFDIR=$(XDGCONFDIR)
else
uninstall:
	rm -f $(addprefix $(PREFIX)/bin/,$(BINARIES))
	rm -f $(addprefix $(MANDIR)/man1/,$(notdir $(wildcard man/*.1)))
	rm -f $(addprefix $(MANDIR)/man7/,$(notdir $(wildcard man/*.7)))
	rm -f $(APPSDIR)/$(SETTINGS_ID).desktop $(APPSDIR)/patina-mimeapps.list $(ICONDIR)/scalable/apps/$(SETTINGS_ID).svg \
		$(APPSDIR)/$(PROCESSES_ID).desktop $(ICONDIR)/scalable/apps/$(PROCESSES_ID).svg
	rm -f $(APPSDIR)/$(FILES_ID).desktop $(ICONDIR)/scalable/apps/$(FILES_ID).svg $(PORTALDIR)/portals/patina.portal \
		$(PREFIX)/share/dbus-1/services/$(PORTAL_SERVICE).service
	rm -f $(APPSDIR)/$(READER_ID).desktop $(ICONDIR)/scalable/apps/$(READER_ID).svg \
		$(APPSDIR)/$(NOTEBOOK_ID).desktop $(ICONDIR)/scalable/apps/$(NOTEBOOK_ID).svg $(ICONDIR)/128x128/apps/$(NOTEBOOK_ID).png
	rm -f $(SESSIONDIR)/patina.desktop $(PAMDIR)/patina-lock $(PORTALCONF) $(WLRCONF)
	-update-desktop-database -q $(APPSDIR)
endif

vm: ## Boot the FreeBSD test VM: the compositor in a window on dwl (bsdt up)
	bsdt up

vm-restart: ## Rebuild in the VM and restart its desktop
	bsdt exec -- cargo build --release
	bsdt gui --restart

vm-gui: ## Open the VM's desktop in a VNC viewer
	bsdt gui

screenshot: ## Save the FreeBSD VM's desktop to docs/screenshot.png (the README's: make vm-linux-screenshot)
	@mkdir -p docs
	bsdt exec -- sh -c '$(VM_WAYLAND) grim /tmp/patina-desktop.png'
	bsdt exec -- cat /tmp/patina-desktop.png > docs/screenshot.png

vm-linux: build ## Boot the Linux test VM: a Patina session on tty1, on the binaries built here (ADR-0017)
	vm-linux/vm.sh start

vm-linux-restart: build ## Rebuild here and restart the Linux VM's session
	vm-linux/vm.sh restart

vm-linux-shot: ## Save the Linux VM's screen to target/vm-linux/screen.png
	vm-linux/vm.sh shot

vm-linux-screenshot: ## Retake the README's screenshot, docs/screenshot.png, in the Linux VM (vm-linux/screenshot.sh)
	vm-linux/screenshot.sh

vm-linux-gui: ## Open the Linux VM's screen in a VNC viewer
	vm-linux/vm.sh view

test-vm: build ## Regression tests against the Linux VM: the session, the bar's battery and Wi-Fi, the apps (needs uv; boots the VM)
	uv run --project vm-linux/tests pytest vm-linux/tests

test-vm-rc: ## Regression tests against the clean rc VM that runs the installed .deb (needs uv; no build)
	PATINA_VM_INSTANCE=rc uv run --project vm-linux/tests pytest vm-linux/tests

vm-linux-stop: ## Power the Linux VM off (vm-linux/vm.sh reset deletes its disk)
	vm-linux/vm.sh stop

clean: ## Remove build output
	cargo clean
