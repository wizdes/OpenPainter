#!/usr/bin/env bash
# Builds the Rust staticlib the app links: engine/target/app-release/libop_ffi.a
# (the app-release cargo profile: release with fat LTO, engine/Cargo.toml).
# Runs with a clean environment so an Xcode build and `make -C mac engine` use
# the same flags (Xcode exports SDK and arch variables that change cargo's
# fingerprints) and so the C parts (lcms2) target macOS 15 like the app.
#
# M58: `cargo rustc --crate-type staticlib` builds only the staticlib. op-ffi
# lists both staticlib and rlib (the rlib is for its Rust tests), and with an
# rlib in the list cargo does not run LTO at all: the profile's `lto` setting
# then has no effect and the archive is about 73 MB instead of about 19 MB.
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
  cargo rustc -p op-ffi --profile app-release --crate-type staticlib
