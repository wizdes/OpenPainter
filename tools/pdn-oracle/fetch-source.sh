#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
src="$root/.pdn-src"
revision=cca476b0df2a2f70996e6b9486ec45327631568c
if [[ -e "$src/.git" ]]; then
  current="$(git -C "$src" rev-parse HEAD)"
  if [[ "$current" != "$revision" ]]; then
    echo "Refusing .pdn-src at $current; expected $revision" >&2
    exit 1
  fi
elif [[ -d "$src" && -n "$(ls -A "$src")" ]]; then
  echo "Refusing nonempty .pdn-src without its own Git repository" >&2
  exit 1
else
  git clone https://github.com/rivy/OpenPDN "$src"
  git -C "$src" checkout --detach "$revision"
fi
test "$(git -C "$src" rev-parse HEAD)" = "$revision"
echo "OpenPDN $revision"
