.DEFAULT_GOAL := help

# The workspace builds into the repository's target/, not the crate's.
TARGET ?= ../target

# A throwaway config dir, so screenshots never touch your settings.
SCREENSHOT_ENV := XDG_CONFIG_HOME=$(CURDIR)/target/screenshot-config XDG_DATA_HOME=$(CURDIR)/target/screenshot-data

.PHONY: help demo-notes run dev build test check icons screenshot clean icon

help: ## Show this help
	@echo "Usage: make <target>"
	@echo
	@awk 'BEGIN { FS = ":.*## " } /^[a-z-]+:.*## / { printf "  \033[1m%-14s\033[0m %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

# The example notes are copied first, so trying things out never edits them.
DEMO := target/demo-notes

demo-notes:
	@rm -rf $(DEMO) && mkdir -p target && cp -r examples/notes $(DEMO)

run: demo-notes ## Run Notebook on a copy of the example notes
	cargo run -- $(DEMO)

dev: demo-notes ## Run with live reload (requires cargo-watch)
	@command -v cargo-watch >/dev/null 2>&1 || { echo "cargo-watch is required: cargo install cargo-watch"; exit 1; }
	cargo watch -x "run -- $(DEMO)"

build: ## Build a release binary
	cargo build --release

test: ## Run the tests
	cargo test

check: test ## Run the tests and clippy
	cargo clippy --all-targets -- -D warnings

icons: ## Render assets/notebook-icon-128.png from packaging/linux/notebook.svg
	cargo run -q --example render_icon_notebook -- 128 assets/notebook-icon-128.png

icon: ## docs/icon.png: the app icon at 120 px, for the README
	scripts/icon.sh

screenshot: ## Regenerate docs/screenshot-*.png from the example notes
	@rm -rf target/screenshot-config target/screenshot-data
	@mkdir -p docs target/screenshot-config target/screenshot-data
	$(SCREENSHOT_ENV) NOTEBOOK_SCREENSHOT=docs/screenshot-notebook.png cargo run -- examples/notes
	$(SCREENSHOT_ENV) NOTEBOOK_SCREENSHOT=docs/screenshot-light.png NOTEBOOK_SKIN=Default NOTEBOOK_THEME=light cargo run -- examples/notes
	$(SCREENSHOT_ENV) NOTEBOOK_SCREENSHOT=docs/screenshot-dark-orange.png NOTEBOOK_SKIN="Dark Orange" cargo run -- examples/notes

clean: ## Remove build output
	cargo clean
