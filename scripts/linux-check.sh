#!/usr/bin/env bash
set -euo pipefail

if ! command -v limactl >/dev/null 2>&1; then
  echo "SKIP Linux check: limactl is missing"
  exit 0
fi
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
printf -v repo_quoted '%q' "$repo_root"
limactl shell op-linux -- bash -lc "cd $repo_quoted/engine && export CARGO_HOME=/tmp/op-linux-cargo CARGO_TARGET_DIR=/tmp/op-linux-target && cargo xtask check --tier fast"
