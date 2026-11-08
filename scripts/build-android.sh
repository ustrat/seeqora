#!/usr/bin/env bash
# Build the Rust core for Android (.so per ABI into jniLibs) and generate
# Kotlin bindings. Run on macOS or Linux with the Android NDK installed.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

export ANDROID_NDK_HOME="${ANDROID_NDK_HOME:-${ANDROID_NDK_LATEST_HOME:-}}"
if [ -z "$ANDROID_NDK_HOME" ]; then
  echo "Set ANDROID_NDK_HOME to your NDK path" >&2
  exit 1
fi

echo "==> Rust targets"
rustup target add aarch64-linux-android x86_64-linux-android
command -v cargo-ndk >/dev/null 2>&1 || cargo install cargo-ndk --locked

echo "==> Build core .so (arm64-v8a, x86_64)"
cargo ndk -t arm64-v8a -t x86_64 -o android/app/src/main/jniLibs \
  build -p seeqora-core --release --lib

echo "==> Generate Kotlin bindings"
cargo build -p seeqora-core --release --lib
case "$(uname -s)" in
  Darwin) HOSTLIB=target/release/libseeqora_core.dylib ;;
  *)      HOSTLIB=target/release/libseeqora_core.so ;;
esac
rm -rf android/app/src/main/generated
cargo run -q -p uniffi-bindgen -- generate \
  --library "$HOSTLIB" --language kotlin \
  --out-dir android/app/src/main/generated

echo "==> Done. Build with: gradle -p android assembleDebug"
