# netrace developer commands. Run `make help` for the list.

# Homebrew's rustup is keg-only, so cargo is often not on PATH. Export the
# directory for cargo's own subcommands (fmt, clippy) and resolve cargo itself
# up front, since make looks up simple commands before applying PATH.
export PATH := /opt/homebrew/opt/rustup/bin:$(PATH)
CARGO := $(shell command -v cargo 2>/dev/null || echo /opt/homebrew/opt/rustup/bin/cargo)

XCODEBUILD = xcodebuild -project app/Netrace.xcodeproj -scheme Netrace -derivedDataPath build -quiet
APP = build/Build/Products/Debug/Netrace.app

.PHONY: help core core-release project app run open test lint clean

help:
	@echo "make core          Build Rust core (debug) + Swift bindings"
	@echo "make core-release  Build universal optimised core"
	@echo "make project       Generate app/Netrace.xcodeproj from app/project.yml"
	@echo "make app           Build the macOS app (Debug)"
	@echo "make run           Build and launch the app"
	@echo "make open          Prepare everything and open the project in Xcode"
	@echo "make test          Run Rust tests"
	@echo "make lint          cargo fmt --check + clippy"
	@echo "make clean         Remove all build output"

core:
	scripts/build-core.sh

core-release:
	scripts/build-core.sh --release

project:
	cd app && xcodegen generate --quiet

app: core project
	$(XCODEBUILD) -configuration Debug build

run: app
	open $(APP)

open: core project
	open app/Netrace.xcodeproj

test:
	$(CARGO) test --workspace

lint:
	$(CARGO) fmt --all --check
	$(CARGO) clippy --workspace --all-targets -- -D warnings

clean:
	$(CARGO) clean
	rm -rf build app/Frameworks app/Netrace/Core/Generated app/Netrace.xcodeproj
