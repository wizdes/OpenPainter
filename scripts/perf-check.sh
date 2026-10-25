#!/usr/bin/env bash
# One command for the M58 speed and size gates. Exit 0 only when every check passes.
#
# Usage: scripts/perf-check.sh [--no-build] [--runs N] [--record-baseline]
#
# 1. Builds the Release app fresh (`make -C mac app`) unless --no-build.
# 2. scripts/size-report.sh: app bundle under perf/budgets.toml app_bundle_mb.
# 3. scripts/launch-time.sh --first-effect (20 runs by default): median launch
#    to a usable canvas under cold_launch_ms, no effect pipeline before
#    OP_READY, first GPU effect on the GPU. Median and p95 are printed.
# 4. Regression limits: launch median and bundle size may not grow more than
#    regression_pct (10%) over perf/baseline.toml. The baseline names the
#    machine it was recorded on; on another machine the check fails until a
#    baseline is recorded there (numbers from different Macs do not compare).
# --record-baseline writes perf/baseline.toml from this run (after checks 2
# and 3 pass) instead of comparing.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
build=1
record=0
runs=20
while (( $# > 0 )); do
  case "$1" in
    --no-build) build=0; shift ;;
    --record-baseline) record=1; shift ;;
    --runs) runs="${2:-}"; shift 2 ;;
    *) echo "Usage: scripts/perf-check.sh [--no-build] [--runs N] [--record-baseline]" >&2; exit 2 ;;
  esac
done
app="$repo_root/mac/build/DerivedData/Build/Products/Release/OpenPainter.app"
baseline="$repo_root/perf/baseline.toml"
budgets="$repo_root/perf/budgets.toml"
value() {  # file key -> value without quotes
  awk -F '=' -v key="$2" '$1 ~ "^[[:space:]]*" key "[[:space:]]*$" {v = $2; gsub(/^[[:space:]]+|[[:space:]]+$/, "", v); gsub(/"/, "", v); print v}' "$1"
}
machine="$(sysctl -n hw.model) $(sysctl -n machdep.cpu.brand_string)"

if (( build )); then
  echo "== Building the Release app (make -C mac app)"
  mkdir -p "$repo_root/mac/build"
  make -C "$repo_root/mac" app > "$repo_root/mac/build/perf-check-build.log" 2>&1 || {
    tail -30 "$repo_root/mac/build/perf-check-build.log" >&2
    echo "FAIL build: see mac/build/perf-check-build.log" >&2
    exit 1
  }
  echo "Built $app"
elif [[ ! -d "$app" ]]; then
  echo "FAIL: no Release app at $app (run without --no-build)" >&2
  exit 1
fi

status=0
echo "== Size"
size_out="$("$repo_root/scripts/size-report.sh" "$app")" || status=1
echo "$size_out"
size_mb="$(echo "$size_out" | awk '/^App bundle:/ {print $3}')"

echo "== Launch"
numbers="$(mktemp -t op-perf-check)"
trap 'rm -f "$numbers"' EXIT
"$repo_root/scripts/launch-time.sh" --first-effect --runs "$runs" --out "$numbers" "$app" || status=1
launch_median="$(value "$numbers" launch_median_ms)"
launch_p95="$(value "$numbers" launch_p95_ms)"
effect_median="$(value "$numbers" first_effect_median_ms)"
effect_p95="$(value "$numbers" first_effect_p95_ms)"
purged="$(value "$numbers" purged)"

if (( status != 0 )); then
  echo "FAIL perf-check: a budget check failed (see above)" >&2
  exit 1
fi

commit="$(git -C "$repo_root" rev-parse --short HEAD 2>/dev/null || echo unknown)"
if (( record )); then
  cat > "$baseline" <<EOF
# Perf baseline for scripts/perf-check.sh (M58). Recorded by
# \`scripts/perf-check.sh --record-baseline\`; regressions are measured against
# these numbers on the same machine only.
machine = "$machine"
recorded = "$(date -u +%Y-%m-%dT%H:%MZ)"
commit = "$commit"
runs = $runs
cache_purged = $purged
launch_median_ms = $launch_median
launch_p95_ms = $launch_p95
first_effect_median_ms = $effect_median
first_effect_p95_ms = $effect_p95
app_bundle_mb = $size_mb
EOF
  echo "Recorded $baseline"
  echo "PASS perf-check (baseline recorded)"
  exit 0
fi

echo "== Regression limits (perf/baseline.toml)"
if [[ ! -f "$baseline" ]]; then
  echo "FAIL regression: no perf/baseline.toml; run scripts/perf-check.sh --record-baseline" >&2
  exit 1
fi
base_machine="$(value "$baseline" machine)"
if [[ "$base_machine" != "$machine" ]]; then
  echo "FAIL regression: the baseline is from \"$base_machine\", this is \"$machine\"; record one here with --record-baseline" >&2
  exit 1
fi
pct="$(value "$budgets" regression_pct)"
check() {  # name measured base unit
  local limit
  limit="$(awk -v b="$3" -v p="$pct" 'BEGIN {printf "%.2f", b * (1 + p / 100)}')"
  if awk -v m="$2" -v l="$limit" 'BEGIN {exit !(m > l)}'; then
    echo "FAIL regression: $1 $2 $4 is more than $pct% over the baseline $3 $4 (limit $limit $4)" >&2
    return 1
  fi
  echo "PASS regression: $1 $2 $4 (baseline $3 $4, limit $limit $4)"
}
check "launch median" "$launch_median" "$(value "$baseline" launch_median_ms)" ms || status=1
check "app bundle" "$size_mb" "$(value "$baseline" app_bundle_mb)" MB || status=1
echo "Summary: launch median $launch_median ms, p95 $launch_p95 ms; first GPU effect median $effect_median ms, p95 $effect_p95 ms; bundle $size_mb MB"
if (( status != 0 )); then
  echo "FAIL perf-check" >&2
  exit 1
fi
echo "PASS perf-check"
