#!/usr/bin/env bash
# Builds the Rust staticlib the app links: engine/target/release/libop_ffi.a.
# Runs with a clean environment so an Xcode build and `make -C mac engine` use
# the same flags (Xcode exports SDK and arch variables that change cargo's
# fingerprints) and so the C parts (lcms2) target macOS 15 like the app.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
engine="$here/../../engine"
toolchain="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin"
cd "$engine"
exec env -i \
  HOME="$HOME" \
  PATH="$toolchain:$HOME/.cargo/bin:/usr/bin:/bin:/usr/sbin:/sbin" \
  MACOSX_DEPLOYMENT_TARGET=15.0 \
  CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}" \
  cargo build -p op-ffi --release
