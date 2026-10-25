#!/usr/bin/env bash
# Builds engine/target/release/opcli, the scenario runner the M28 end-to-end
# test (Phase2EndToEndTests) runs. Same clean environment as build-engine.sh,
# so both builds share cargo's fingerprints.
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
  cargo build -p op-cli --release
