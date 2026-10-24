#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if (( $# == 0 )); then
  echo "SKIP size report: no app path"
  exit 0
fi
if (( $# != 1 )); then
  echo "Usage: scripts/size-report.sh path/to/App.app" >&2
  exit 2
fi
app_path="$1"
if [[ ! -d "$app_path" || "$app_path" != *.app ]]; then
  echo "Expected an existing .app directory: $app_path" >&2
  exit 2
fi
budget="$(awk -F '=' '$1 ~ /^[[:space:]]*app_bundle_mb[[:space:]]*$/ {gsub(/[[:space:]]/, "", $2); print $2}' "$repo_root/perf/budgets.toml")"
if [[ ! "$budget" =~ ^[0-9]+([.][0-9]+)?$ ]]; then
  echo "Invalid app_bundle_mb budget" >&2
  exit 2
fi
size_kb="$(du -sk "$app_path" | awk '{print $1}')"
size_mb="$(awk -v kb="$size_kb" 'BEGIN {printf "%.2f", kb / 1024}')"
echo "App bundle: $size_mb MB; budget: $budget MB"
if awk -v kb="$size_kb" -v limit="$budget" 'BEGIN {exit !(kb / 1024 > limit)}'; then
  echo "FAIL size report: bundle exceeds budget" >&2
  exit 1
fi
echo "PASS size report"
