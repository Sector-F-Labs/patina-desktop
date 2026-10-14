VERSION := $(shell awk -F\" '/^version = / { print $$2; exit }' Cargo.toml)
PREFIX ?= $(HOME)/.local

# grim inside the VM's dwl session, which bsdt runs headless.
VM_WAYLAND := XDG_RUNTIME_DIR=/tmp/bsdt-xdg WAYLAND_DISPLAY=wayland-0

.DEFAULT_GOAL := help

.PHONY: help run run-bar build test check install uninstall install-session uninstall-session vm vm-restart vm-gui screenshot clean

help: ## Show this help
	@echo "Usage: make <target>"
	@echo
	@awk 'BEGIN { FS = ":.*## " } /^[a-z-]+:.*## / { printf "  \033[1m%-14s\033[0m %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

run: ## Run the compositor in a window on this desktop, with the bar
	cargo build --workspace
	target/debug/patina-compositor

run-bar: ## Run only the bar on the current compositor (needs layer-shell; not GNOME)
	cargo run -p patina-bar < /dev/null

build: ## Build a release binary
	cargo build --release

test: ## Run the tests
	cargo test --workspace

check: test ## Run the tests and clippy, as before a commit or release
	cargo clippy --workspace --all-targets -- -D warnings

BINARIES := patina-bar patina-compositor patina-launcher

install: build ## Install the binaries into $(PREFIX)/bin
	install -d $(PREFIX)/bin
	install -m 755 $(addprefix target/release/,$(BINARIES)) $(PREFIX)/bin/

uninstall: ## Remove the installed binaries
	rm -f $(addprefix $(PREFIX)/bin/,$(BINARIES))

# Login sessions are only read from the system folder, so this one file
# needs root. The binaries stay per user; run these without sudo, or
# PREFIX would be root's home.
SESSION_DIR := /usr/share/wayland-sessions

install-session: install ## Also add "Patina" to the login screen's sessions (asks for sudo)
	@test "$$(id -u)" != 0 || { echo "Run make install-session without sudo; it asks for sudo itself."; exit 1; }
	@mkdir -p target
	sed 's|@BINDIR@|$(PREFIX)/bin|' packaging/linux/patina.desktop > target/patina.desktop
	sudo install -D -m 644 target/patina.desktop $(SESSION_DIR)/patina.desktop
	@echo "Installed $(SESSION_DIR)/patina.desktop. Running on the hardware isn't built yet,"
	@echo "so choosing Patina at login returns to the login screen until it is."

uninstall-session: ## Remove "Patina" from the login screen's sessions (asks for sudo)
	@test "$$(id -u)" != 0 || { echo "Run make uninstall-session without sudo; it asks for sudo itself."; exit 1; }
	sudo rm -f $(SESSION_DIR)/patina.desktop

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
