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

# llama.cpp (answers): build its arm64 kernels for ARMv8.2 int8 dot product
# and the int8 matrix-multiply extension, which do most of the prompt-reading
# work. The app checks /proc/cpuinfo for both before loading the core.
export GGML_CPU_ARM_ARCH="armv8.2-a+dotprod+i8mm"
export ANDROID_NDK="$ANDROID_NDK_HOME"
export ANDROID_PLATFORM="android-29"  # app minSdk

# --platform matches minSdk: cargo-ndk's compiler wrapper otherwise targets
# API 21, where bionic lacks posix_madvise (llama.cpp needs it, API 23+).
echo "==> Build core .so (arm64-v8a, x86_64)"
cargo ndk --platform 29 -t arm64-v8a -t x86_64 -o android/app/src/main/jniLibs \
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
