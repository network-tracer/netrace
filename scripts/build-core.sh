#!/usr/bin/env bash
# Builds the Rust core into NetraceCore.xcframework and generates the Swift
# bindings the app compiles against.
#
#   scripts/build-core.sh              debug, this Mac's architecture only
#   scripts/build-core.sh --release    optimised, universal (arm64 + x86_64)
#
# Outputs (both git-ignored):
#   app/Frameworks/NetraceCore.xcframework
#   app/Netrace/Core/Generated/netrace_ffi.swift
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

# Homebrew's rustup is keg-only, so its cargo is often not on PATH.
if ! command -v cargo >/dev/null 2>&1 && [ -x /opt/homebrew/opt/rustup/bin/cargo ]; then
  export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
fi
command -v cargo >/dev/null 2>&1 || { echo "error: cargo not found. Install Rust with rustup." >&2; exit 1; }

PROFILE=debug
CARGO_PROFILE_FLAG=()
case "${1:-}" in
  --release) PROFILE=release; CARGO_PROFILE_FLAG=(--release) ;;
  "") ;;
  *) echo "usage: $0 [--release]" >&2; exit 2 ;;
esac

if [ "$PROFILE" = release ]; then
  TARGETS=(aarch64-apple-darwin x86_64-apple-darwin)
else
  case "$(uname -m)" in
    arm64) TARGETS=(aarch64-apple-darwin) ;;
    *)     TARGETS=(x86_64-apple-darwin) ;;
  esac
fi

for t in "${TARGETS[@]}"; do
  if ! rustup target list --installed | grep -qx "$t"; then
    echo "error: Rust target $t is missing. Run: rustup target add $t" >&2
    exit 1
  fi
done

# Must match MACOSX_DEPLOYMENT_TARGET in app/project.yml.
export MACOSX_DEPLOYMENT_TARGET=15.0

LIB=libnetrace_ffi
BUILD_DIR="$ROOT/target/xcframework"
HEADERS_DIR="$BUILD_DIR/include"
OUT_FRAMEWORK="$ROOT/app/Frameworks/NetraceCore.xcframework"
OUT_SWIFT_DIR="$ROOT/app/Netrace/Core/Generated"

echo "==> Building Rust core ($PROFILE: ${TARGETS[*]})"
for t in "${TARGETS[@]}"; do
  cargo build -p netrace-ffi --lib --target "$t" "${CARGO_PROFILE_FLAG[@]}"
done

echo "==> Generating Swift bindings"
rm -rf "$BUILD_DIR"
mkdir -p "$HEADERS_DIR" "$OUT_SWIFT_DIR"
# Library mode reads the exported API from the compiled cdylib's metadata.
cargo run -q -p netrace-ffi --features bindgen --bin uniffi-bindgen-swift -- \
  "target/${TARGETS[0]}/$PROFILE/$LIB.dylib" "$BUILD_DIR/bindings" \
  --swift-sources --headers --modulemap \
  --module-name netrace_ffiFFI --modulemap-filename module.modulemap
mv "$BUILD_DIR/bindings/"*.h "$BUILD_DIR/bindings/module.modulemap" "$HEADERS_DIR/"
mv "$BUILD_DIR/bindings/netrace_ffi.swift" "$OUT_SWIFT_DIR/netrace_ffi.swift"

echo "==> Packaging NetraceCore.xcframework"
if [ "${#TARGETS[@]}" -gt 1 ]; then
  STATIC_LIB="$BUILD_DIR/$LIB.a"
  lipo -create $(for t in "${TARGETS[@]}"; do echo "target/$t/$PROFILE/$LIB.a"; done) -output "$STATIC_LIB"
else
  STATIC_LIB="target/${TARGETS[0]}/$PROFILE/$LIB.a"
fi
rm -rf "$OUT_FRAMEWORK"
mkdir -p "$(dirname "$OUT_FRAMEWORK")"
xcodebuild -create-xcframework -library "$STATIC_LIB" -headers "$HEADERS_DIR" \
  -output "$OUT_FRAMEWORK" >/dev/null

echo "==> Done"
echo "    $OUT_FRAMEWORK"
echo "    $OUT_SWIFT_DIR/netrace_ffi.swift"
