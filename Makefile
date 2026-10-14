VERSION := $(shell awk -F\" '/^version = / { print $$2; exit }' Cargo.toml)
PREFIX ?= $(HOME)/.local

# grim inside the VM's dwl session, which bsdt runs headless.
VM_WAYLAND := XDG_RUNTIME_DIR=/tmp/bsdt-xdg WAYLAND_DISPLAY=wayland-0

.DEFAULT_GOAL := help

.PHONY: help run run-bar build test check install uninstall vm vm-restart vm-gui screenshot clean

help: ## Show this help
	@echo "Usage: make <target>"
	@echo
	@awk 'BEGIN { FS = ":.*## " } /^[a-z-]+:.*## / { printf "  \033[1m%-14s\033[0m %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

run: ## Run the compositor in a window on this desktop, with the bar and foot
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

install: build ## Install both binaries into $(PREFIX)/bin
	install -d $(PREFIX)/bin
	install -m 755 target/release/patina-bar target/release/patina-compositor $(PREFIX)/bin/

uninstall: ## Remove the installed binaries
	rm -f $(PREFIX)/bin/patina-bar $(PREFIX)/bin/patina-compositor

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
