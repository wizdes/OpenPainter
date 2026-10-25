#!/usr/bin/env bash
# Runs a check tier inside the Lima VM `op-linux`.
# The VM's view of the shared home folder can lag behind (cached metadata),
# so cargo there may see old files and report a stale pass. Instead, copy the
# tracked and new files into a VM-local folder, and compare a hash of the
# source tree on both sides before testing.
set -euo pipefail

tier="${1:-fast}"
if ! command -v limactl >/dev/null 2>&1; then
  echo "SKIP Linux check: limactl is missing"
  exit 0
fi
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

files="$(git ls-files -co --exclude-standard -- engine scripts tests tools docs/reports perf | while read -r f; do [ -f "$f" ] && printf '%s\n' "$f"; done)"
mac_hash="$(printf '%s\n' "$files" | tr '\n' '\0' | xargs -0 shasum -a 256 | shasum -a 256 | cut -d' ' -f1)"

dest=/tmp/op-linux-src
printf '%s\n' "$files" | tr '\n' '\0' | COPYFILE_DISABLE=1 tar --null -T - -cf - |
  limactl shell op-linux -- bash -c "rm -rf $dest && mkdir -p $dest && tar -xf - -C $dest"

vm_hash="$(printf '%s\n' "$files" | limactl shell op-linux -- bash -c "cd $dest && tr '\n' '\0' | xargs -0 sha256sum | sha256sum | cut -d' ' -f1")"
if [ "$mac_hash" != "$vm_hash" ]; then
  echo "FAIL Linux check: source in the VM differs from the Mac (mac $mac_hash, vm $vm_hash)"
  exit 1
fi
echo "Linux source hash matches: $mac_hash"

limactl shell op-linux -- bash -lc "cd $dest/engine && export CARGO_HOME=/tmp/op-linux-cargo CARGO_TARGET_DIR=/tmp/op-linux-target VK_DRIVER_FILES=/usr/share/vulkan/icd.d/lvp_icd.json && cargo xtask check --tier $tier"
