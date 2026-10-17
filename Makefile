VERSION := $(shell awk -F\" '/^version = / { print $$2; exit }' Cargo.toml)
PREFIX ?= $(HOME)/.local
# Under $(PREFIX)/share/man, man(1) finds the pages through PATH's bin folder.
MANDIR ?= $(PREFIX)/share/man

# grim inside the VM's dwl session, which bsdt runs headless.
VM_WAYLAND := XDG_RUNTIME_DIR=/tmp/bsdt-xdg WAYLAND_DISPLAY=wayland-0

.DEFAULT_GOAL := help

.PHONY: help deps run run-bar build test check install uninstall install-session uninstall-session vm vm-restart vm-gui screenshot clean

help: ## Show this help
	@echo "Usage: make <target>"
	@echo
	@awk 'BEGIN { FS = ":.*## " } /^[a-z-]+:.*## / { printf "  \033[1m%-14s\033[0m %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

deps: ## Show the system packages the build needs
	@echo "Ubuntu/Debian: sudo apt install pkg-config libxkbcommon-dev libpixman-1-dev libudev-dev \\"
	@echo "                   libinput-dev libseat-dev libgbm-dev libegl-dev libdrm-dev libwayland-dev"
	@echo "FreeBSD:       pkg install pkgconf libxkbcommon pixman libudev-devd libinput seatd mesa-libs libdrm wayland"
	@echo "To run X11 apps (Steam), Xwayland too: apt install xwayland, or pkg install xwayland."

run: ## Run the compositor in a window on this desktop, with the bar
	cargo build --workspace
	target/debug/patina-compositor

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

BINARIES := patina-bar patina-compositor patina-lock patina-menu patina-night-light patina-power patina-session patina-settings patina-tiler patina-volume patina-wallpaper patina-wifi
# The launcher lives in its own repository (ADR-0037); install builds it from there.
LAUNCHER := https://github.com/Sector-F-Labs/patina-launcher

install: build ## Install the binaries into $(PREFIX)/bin, and their man pages into $(MANDIR)
	install -d $(PREFIX)/bin $(MANDIR)/man1 $(MANDIR)/man7
	install -m 755 $(addprefix target/release/,$(BINARIES)) $(PREFIX)/bin/
	install -m 644 man/*.1 $(MANDIR)/man1/
	install -m 644 man/*.7 $(MANDIR)/man7/
	cargo install --quiet --locked --force --git $(LAUNCHER) --root $(PREFIX) patina-launcher

uninstall: ## Remove the installed binaries and man pages
	rm -f $(addprefix $(PREFIX)/bin/,$(BINARIES))
	rm -f $(addprefix $(MANDIR)/man1/,$(notdir $(wildcard man/*.1)))
	rm -f $(addprefix $(MANDIR)/man7/,$(notdir $(wildcard man/*.7)))
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
