.DEFAULT_GOAL := help

# The workspace builds into the repository's target/, not the crate's.
TARGET ?= ../target

APP_ID := com.sectorflabs.Files

.PHONY: help run build test check default app clean screenshots icon

help: ## Show this help
	@echo "Usage: make <target>"
	@echo
	@awk 'BEGIN { FS = ":.*## " } /^[a-z-]+:.*## / { printf "  \033[1m%-14s\033[0m %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

run: ## Run Files
	cargo run

build: ## Build a release binary
	cargo build --release

test: ## Run the tests
	cargo test

screenshots: build ## Rewrite docs/screenshots on made-up data (docs/demo/)
	docs/demo/screenshots.sh

icon: ## docs/icon.png: the app icon at 120 px, for the README
	cargo run -q --example render_icon_files -- 120 docs/icon.png

check: test ## Run the tests and clippy
	cargo clippy --all-targets -- -D warnings

default: ## Make Files the app that opens folders (xdg-mime)
	xdg-mime default $(APP_ID).desktop inode/directory
	@echo "Folders now open in Files; 'xdg-mime default org.gnome.Nautilus.desktop inode/directory' undoes it"

app: build ## Make target/Files.app, a folder app for ~/Apps
	rm -rf target/Files.app && mkdir -p target/Files.app/bin
	install -m755 $(TARGET)/release/files target/Files.app/bin/files
	install -m644 packaging/linux/files.svg target/Files.app/icon.svg
	printf 'format = 1\nname = "Files"\nexec = "bin/files"\nid = "$(APP_ID)"\nsummary = "File Manager"\nkeywords = ["folder", "file manager"]\ncategories = ["System", "Utility"]\nicon = "icon.svg"\n' > target/Files.app/app.toml
	@echo "Made target/Files.app"

clean: ## Remove build output
	cargo clean
