#!/usr/bin/env bash
# Regression limits for scripts/perf-check.sh (M58): compares one run's
# numbers with a recorded baseline. Exit 0 only when every number is within
# perf/budgets.toml regression_pct (10%) of the baseline.
#
# Usage: scripts/perf-compare.sh NUMBERS BASELINE
#
# Both files are `key = value` lines: machine, cache_purged, launch_median_ms,
# launch_p95_ms, first_effect_median_ms, app_bundle_mb. It refuses (fails) to
# compare numbers from another machine, or a run with the file cache purged
# against a baseline without it (or the other way round): those numbers do not
# measure the same thing.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if (( $# != 2 )); then
  echo "Usage: scripts/perf-compare.sh NUMBERS BASELINE" >&2
  exit 2
fi
numbers="$1"
baseline="$2"
if [[ ! -f "$numbers" ]]; then
  echo "FAIL regression: no numbers file $numbers" >&2
  exit 1
fi
if [[ ! -f "$baseline" ]]; then
  echo "FAIL regression: no baseline $baseline; run scripts/perf-check.sh --record-baseline" >&2
  exit 1
fi
value() {  # file key -> value without quotes
  awk -F '=' -v key="$2" '$1 ~ "^[[:space:]]*" key "[[:space:]]*$" {v = $2; gsub(/^[[:space:]]+|[[:space:]]+$/, "", v); gsub(/"/, "", v); print v}' "$1"
}
machine="$(value "$numbers" machine)"
base_machine="$(value "$baseline" machine)"
if [[ "$base_machine" != "$machine" ]]; then
  echo "FAIL regression: the baseline is from \"$base_machine\", this is \"$machine\"; record one here with --record-baseline" >&2
  exit 1
fi
purged="$(value "$numbers" cache_purged)"
base_purged="$(value "$baseline" cache_purged)"
if [[ "$purged" != "$base_purged" ]]; then
  describe() { [[ "$1" == 1 ]] && echo "with the file cache purged" || echo "without purging the file cache"; }
  echo "FAIL regression: this run was measured $(describe "$purged") but the baseline $(describe "$base_purged"); they do not compare. Record a baseline in the same mode." >&2
  exit 1
fi
pct="$(value "$repo_root/perf/budgets.toml" regression_pct)"
status=0
check() {  # name key unit
  local measured base limit
  measured="$(value "$numbers" "$2")"
  base="$(value "$baseline" "$2")"
  if [[ ! "$measured" =~ ^[0-9]+([.][0-9]+)?$ || ! "$base" =~ ^[0-9]+([.][0-9]+)?$ ]]; then
    echo "FAIL regression: $1 is missing or not a number (run: \"$measured\", baseline: \"$base\")" >&2
    return 1
  fi
  limit="$(awk -v b="$base" -v p="$pct" 'BEGIN {printf "%.2f", b * (1 + p / 100)}')"
  if awk -v m="$measured" -v l="$limit" 'BEGIN {exit !(m > l)}'; then
    echo "FAIL regression: $1 $measured $3 is more than $pct% over the baseline $base $3 (limit $limit $3)" >&2
    return 1
  fi
  echo "PASS regression: $1 $measured $3 (baseline $base $3, limit $limit $3)"
}
check "launch median" launch_median_ms ms || status=1
check "launch p95" launch_p95_ms ms || status=1
check "first GPU effect median" first_effect_median_ms ms || status=1
check "app bundle" app_bundle_mb MB || status=1
exit "$status"
