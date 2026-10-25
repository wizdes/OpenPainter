#!/usr/bin/env bash
# Cold launch time of the Release app (M12, extended in M58).
#
# Usage: scripts/launch-time.sh [--first-effect] [--runs N] [--document FILE]
#                               [--out FILE] path/to/OpenPainter.app
#
# Each run starts the app binary with one document (default
# tests/fixtures/plasma_512x384.png) and reads its stdout:
#   OP_READY <ms> effect=<n> view=<n> mip=<n>
#       <ms>: process start to the first presented canvas frame (a usable
#       canvas). The counts: GPU pipelines made before that frame.
#   OP_FIRST_EFFECT <ms> gpu=<0|1>   (only with --first-effect, which sets
#       OP_LAUNCH_PROBE=first-effect): process start to the end of the first
#       GPU effect (a tiny Gaussian blur that starts the effect device and
#       compiles its kernels).
# Then it kills the run (by its PID) and starts the next one.
#
# Cache: when `sudo -n` works without a password, the file cache is purged
# before every run (true cold launches). Otherwise nothing is purged and the
# script says so; perf-check.sh then runs it right after a fresh build.
#
# Fails when: the median OP_READY time is over perf/budgets.toml
# cold_launch_ms; any run made an effect pipeline before OP_READY; with
# --first-effect, a run printed no OP_READY counts, no OP_FIRST_EFFECT line, or
# ran the effect on the CPU. --out writes the numbers as key = value lines.
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
runs=20
first_effect=0
document="$repo_root/tests/fixtures/plasma_512x384.png"
out_file=""
app_path=""
while (( $# > 0 )); do
  case "$1" in
    --first-effect) first_effect=1; shift ;;
    --runs) runs="${2:-}"; shift 2 ;;
    --document) document="${2:-}"; shift 2 ;;
    --out) out_file="${2:-}"; shift 2 ;;
    -*) echo "Unknown option: $1" >&2; exit 2 ;;
    *)
      if [[ -n "$app_path" ]]; then
        echo "Usage: scripts/launch-time.sh [--first-effect] [--runs N] [--document FILE] [--out FILE] path/to/App.app" >&2
        exit 2
      fi
      app_path="$1"; shift ;;
  esac
done
if [[ -z "$app_path" ]]; then
  echo "SKIP launch time: no app path"
  exit 0
fi
if [[ ! "$runs" =~ ^[1-9][0-9]*$ ]]; then
  echo "--runs needs a positive whole number" >&2
  exit 2
fi
if [[ ! -f "$document" ]]; then
  echo "Launch document not found: $document" >&2
  exit 2
fi
if [[ ! -d "$app_path" || "$app_path" != *.app ]]; then
  echo "Expected an existing .app directory: $app_path" >&2
  exit 2
fi
if [[ ! -f "$app_path/Contents/Info.plist" ]]; then
  echo "Missing Contents/Info.plist: $app_path" >&2
  exit 2
fi
if [[ ! -x /usr/libexec/PlistBuddy ]]; then
  echo "PlistBuddy is required on macOS" >&2
  exit 2
fi
binary_name="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$app_path/Contents/Info.plist")"
binary="$app_path/Contents/MacOS/$binary_name"
if [[ ! -x "$binary" ]]; then
  echo "App binary is not executable: $binary" >&2
  exit 2
fi
read_budget() {
  awk -F '=' -v key="$1" '$1 ~ "^[[:space:]]*" key "[[:space:]]*$" {gsub(/[[:space:]]/, "", $2); print $2}' \
    "$repo_root/perf/budgets.toml"
}
budget="$(read_budget cold_launch_ms)"
aim="$(read_budget cold_launch_aim_ms)"
if [[ ! "$budget" =~ ^[0-9]+([.][0-9]+)?$ ]]; then
  echo "Invalid cold_launch_ms budget" >&2
  exit 2
fi

purge=0
if [[ -x /usr/sbin/purge ]] && sudo -n true 2>/dev/null; then
  purge=1
  echo "Launch mode: file cache purged before each run (sudo -n purge)"
else
  echo "Launch mode: file cache NOT purged (purge needs an admin password); run this right after a fresh build"
fi

# The app's stderr goes to one file per run; a failed run prints its tail.
logdir="$(mktemp -d -t op-launch)"
trap 'rm -rf "$logdir"' EXIT
# Prints one line per run: "<ready_ms> <effect_pipelines|-> <first_effect_ms|-> <gpu|->".
results="$(perl -e '
use strict;
use warnings;
use POSIX ":sys_wait_h";
my ($binary, $document, $runs, $first_effect, $purge, $logdir) = @ARGV;
sub stderr_tail {
    my ($run) = @_;
    open(my $f, "<", "$logdir/run-$run.err") or return "";
    my @lines = <$f>;
    return @lines ? "app stderr (last lines):\n" . join("", @lines[-(@lines < 20 ? @lines : 20) .. -1]) : "";
}
$ENV{OP_LAUNCH_PROBE} = "first-effect" if $first_effect;
for my $run (1 .. $runs) {
    system("sudo", "-n", "/usr/sbin/purge") == 0 or die "purge failed\n" if $purge;
    my $pid = open(my $out, "-|");
    defined $pid or die "fork failed: $!\n";
    if ($pid == 0) {
        open(STDERR, ">", "$logdir/run-$run.err") or die "stderr: $!\n";
        exec {$binary} $binary, $document, "-ApplePersistenceIgnoreState", "YES" or die "exec failed: $!\n";
    }
    my ($ready, $effects, $effect_ms, $gpu, $failed);
    eval {
        local $SIG{ALRM} = sub { die "run $run: 30 s launch watchdog expired\n" };
        alarm 30;
        while (my $line = <$out>) {
            if (!defined $ready && $line =~ /^OP_READY ([0-9]+(?:\.[0-9]+)?)(.*)$/) {
                $ready = $1;
                my $rest = $2;
                $effects = $1 if $rest =~ /\beffect=([0-9]+)\b/;
                $failed = "run $run: OP_READY has no pipeline counts:$rest" if $first_effect && !defined $effects;
                last if !$first_effect || defined $failed;
            } elsif ($line =~ /^OP_FIRST_EFFECT ([0-9]+(?:\.[0-9]+)?) gpu=([01])\s*$/) {
                ($effect_ms, $gpu) = ($1, $2);
                last;
            } elsif ($line =~ /^OP_FIRST_EFFECT_FAILED (.*)$/) {
                $failed = "run $run: first effect failed: $1";
                last;
            }
        }
        alarm 0;
    };
    my $error = $@;
    alarm 0;
    kill "KILL", $pid;
    close $out;
    waitpid($pid, 0);
    $error ||= "$failed\n" if defined $failed;
    $error ||= "run $run: app exited without OP_READY <ms>\n" if !defined $ready;
    $error ||= "run $run: app exited without OP_FIRST_EFFECT <ms>\n" if $first_effect && !defined $effect_ms;
    die $error . stderr_tail($run) if $error;
    printf "%s %s %s %s\n", $ready, $effects // "-", $effect_ms // "-", $gpu // "-";
}
' "$binary" "$document" "$runs" "$first_effect" "$purge" "$logdir"
)"

stats() {  # column -> "median p95" (p95 is the nearest-rank value)
  echo "$results" | awk -v c="$1" '{print $c}' | sort -g | awk '
    {v[NR] = $1}
    END {
      m = (NR % 2) ? v[(NR + 1) / 2] : (v[NR / 2] + v[NR / 2 + 1]) / 2
      r = int(NR * 0.95); if (r < NR * 0.95) r++; if (r < 1) r = 1
      print m, v[r]
    }'
}
read -r ready_median ready_p95 < <(stats 1)
echo "Cold launch median: $ready_median ms over $runs runs; budget: $budget ms"
echo "Process start to usable canvas (OP_READY): median $ready_median ms, p95 $ready_p95 ms; aim < ${aim:-?} ms"

status=0
effect_median="-"; effect_p95="-"
max_effect_pipelines="$(echo "$results" | awk '$2 != "-" {if ($2 > m) m = $2; n++} END {if (n) print m + 0; else print "-"}')"
if [[ "$max_effect_pipelines" == "-" ]]; then
  echo "SKIP lazy-pipeline check: the app printed no pipeline counts on OP_READY"
elif (( max_effect_pipelines > 0 )); then
  echo "FAIL lazy pipelines: a launch made $max_effect_pipelines effect pipeline(s) before OP_READY" >&2
  status=1
else
  echo "PASS lazy pipelines: no effect pipeline before OP_READY in any run"
fi
if (( first_effect )); then
  read -r effect_median effect_p95 < <(stats 3)
  echo "Process start to first GPU effect done: median $effect_median ms, p95 $effect_p95 ms"
  if echo "$results" | awk '$4 != "1" {bad = 1} END {exit !bad}'; then
    echo "FAIL first effect: at least one run used the CPU path, not the GPU" >&2
    status=1
  fi
fi
if [[ -n "$out_file" ]]; then
  {
    echo "runs = $runs"
    echo "purged = $purge"
    echo "launch_median_ms = $ready_median"
    echo "launch_p95_ms = $ready_p95"
    echo "first_effect_median_ms = \"$effect_median\""
    echo "first_effect_p95_ms = \"$effect_p95\""
    echo "max_effect_pipelines_before_ready = \"$max_effect_pipelines\""
  } > "$out_file"
fi
if awk -v measured="$ready_median" -v limit="$budget" 'BEGIN {exit !(measured > limit)}'; then
  echo "FAIL launch time: median exceeds budget" >&2
  exit 1
fi
if (( status != 0 )); then
  exit "$status"
fi
echo "PASS launch time"
