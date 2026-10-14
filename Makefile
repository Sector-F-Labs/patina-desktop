APP_NAME := patina-bar
VERSION := $(shell awk -F\" '/^version = / { print $$2; exit }' Cargo.toml)
PREFIX ?= $(HOME)/.local

# grim inside the VM's dwl session, which bsdt runs headless.
VM_WAYLAND := XDG_RUNTIME_DIR=/tmp/bsdt-xdg WAYLAND_DISPLAY=wayland-0

.DEFAULT_GOAL := help

.PHONY: help run build test check install uninstall vm vm-restart vm-gui screenshot clean

help: ## Show this help
	@echo "Usage: make <target>"
	@echo
	@awk 'BEGIN { FS = ":.*## " } /^[a-z-]+:.*## / { printf "  \033[1m%-14s\033[0m %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

run: ## Run on the current compositor (needs layer-shell, e.g. dwl or sway; not GNOME)
	cargo run < /dev/null

build: ## Build a release binary
	cargo build --release

test: ## Run the tests
	cargo test

check: test ## Run the tests and clippy, as before a commit or release
	cargo clippy --all-targets -- -D warnings

install: build ## Install the binary into $(PREFIX)/bin
	install -d $(PREFIX)/bin
	install -m 755 target/release/$(APP_NAME) $(PREFIX)/bin/$(APP_NAME)

uninstall: ## Remove the installed binary
	rm -f $(PREFIX)/bin/$(APP_NAME)

vm: ## Boot the FreeBSD test VM with dwl and the bar (bsdt up)
	bsdt up

vm-restart: ## Rebuild the bar in the VM and restart dwl with it
	bsdt exec -- cargo build --release
	bsdt gui --restart

vm-gui: ## Open the VM's desktop in a VNC viewer
	bsdt gui

screenshot: ## Save the VM's bar to docs/screenshot.png
	@mkdir -p docs
	bsdt exec -- sh -c '$(VM_WAYLAND) grim -g "0,0 1280x26" /tmp/patina-bar.png'
	bsdt exec -- cat /tmp/patina-bar.png > docs/screenshot.png

clean: ## Remove build output
	cargo clean
