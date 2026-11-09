#!/usr/bin/env bash
# Build the Rust core for iOS, generate Swift bindings, package the
# XCFramework, and generate the Xcode project. Run on macOS (MacInCloud or CI).
#
# Needs: Xcode 15+, rustup. XcodeGen is downloaded into ios/build/tools if it
# is not on PATH (no Homebrew or sudo required).
set -euo pipefail

# Match the app's minimum (ios/project.yml). Rust's default iOS 10 target cannot
# link C code built by the current SDK (missing ___chkstk_darwin).
export IPHONEOS_DEPLOYMENT_TARGET=17.0

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

XCODEGEN_VERSION="2.42.0"
TARGETS=(aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios)

echo "==> Rust targets"
rustup target add "${TARGETS[@]}"

echo "==> Build core static libs"
for t in "${TARGETS[@]}"; do
  cargo build -p seeqora-core --release --lib --target "$t"
done

echo "==> Generate Swift bindings"
cargo build -p seeqora-core --release --lib
OUT=ios/build/uniffi
rm -rf "$OUT" && mkdir -p "$OUT"
cargo run -q -p uniffi-bindgen -- generate \
  --library target/release/libseeqora_core.dylib \
  --language swift --out-dir "$OUT"

mkdir -p ios/Seeqora/Generated
cp "$OUT/seeqora_core.swift" ios/Seeqora/Generated/

HDR=ios/build/headers
rm -rf "$HDR" && mkdir -p "$HDR"
cp "$OUT/seeqora_coreFFI.h" "$HDR/"
cp "$OUT/seeqora_coreFFI.modulemap" "$HDR/module.modulemap"

echo "==> Package XCFramework"
mkdir -p ios/build/sim
lipo -create \
  target/aarch64-apple-ios-sim/release/libseeqora_core.a \
  target/x86_64-apple-ios/release/libseeqora_core.a \
  -output ios/build/sim/libseeqora_core.a
rm -rf ios/Frameworks/SeeqoraCore.xcframework
mkdir -p ios/Frameworks
xcodebuild -create-xcframework \
  -library target/aarch64-apple-ios/release/libseeqora_core.a -headers "$HDR" \
  -library ios/build/sim/libseeqora_core.a -headers "$HDR" \
  -output ios/Frameworks/SeeqoraCore.xcframework

echo "==> Generate Xcode project"
if command -v xcodegen >/dev/null 2>&1; then
  XCODEGEN="$(command -v xcodegen)"
else
  TOOLS="$ROOT/ios/build/tools"
  XCODEGEN="$TOOLS/xcodegen/bin/xcodegen"
  if [ ! -x "$XCODEGEN" ]; then
    mkdir -p "$TOOLS"
    curl -fsSL -o "$TOOLS/xcodegen.zip" \
      "https://github.com/yonaskolb/XcodeGen/releases/download/${XCODEGEN_VERSION}/xcodegen.zip"
    unzip -q -o "$TOOLS/xcodegen.zip" -d "$TOOLS"
  fi
fi
(cd ios && "$XCODEGEN" generate)

echo "==> Done. Open ios/Seeqora.xcodeproj"
