# netrace developer commands. Run `make help` for the list.

# Homebrew's rustup is keg-only, so cargo is often not on PATH. Export the
# directory for cargo's own subcommands (fmt, clippy) and resolve cargo itself
# up front, since make looks up simple commands before applying PATH.
export PATH := /opt/homebrew/opt/rustup/bin:$(PATH)
CARGO := $(shell command -v cargo 2>/dev/null || echo /opt/homebrew/opt/rustup/bin/cargo)

XCODEBUILD = xcodebuild -project app/Netrace.xcodeproj -scheme Netrace -derivedDataPath build -quiet
APP = build/Build/Products/Debug/Netrace.app
RELEASE_APP = build/Build/Products/Release/Netrace.app
VERSION = $(shell sed -n 's/^ *MARKETING_VERSION: "\(.*\)"/\1/p' app/project.yml)

.PHONY: help core core-release project app run open test lint beta clean

help:
	@echo "make core          Build Rust core (debug) + Swift bindings"
	@echo "make core-release  Build universal optimised core"
	@echo "make project       Generate app/Netrace.xcodeproj from app/project.yml"
	@echo "make app           Build the macOS app (Debug)"
	@echo "make run           Build and launch the app"
	@echo "make open          Prepare everything and open the project in Xcode"
	@echo "make test          Run Rust tests"
	@echo "make lint          cargo fmt --check + clippy"
	@echo "make beta          Test, then build the universal Release app into dist/"
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

# A beta build: tests first, then the optimised universal core and the
# Release app, zipped with ditto so the bundle's metadata survives.
beta: lint test core-release project
	$(XCODEBUILD) -configuration Release ARCHS="arm64 x86_64" ONLY_ACTIVE_ARCH=NO build
	mkdir -p dist
	rm -f dist/netrace-$(VERSION)-beta.zip
	ditto -c -k --keepParent $(RELEASE_APP) dist/netrace-$(VERSION)-beta.zip
	@echo "dist/netrace-$(VERSION)-beta.zip"

clean:
	$(CARGO) clean
	rm -rf build dist app/Frameworks app/Netrace/Core/Generated app/Netrace.xcodeproj
