#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if (( $# == 0 )); then
  echo "SKIP launch time: no app path"
  exit 0
fi
if (( $# != 1 )); then
  echo "Usage: scripts/launch-time.sh path/to/App.app" >&2
  exit 2
fi
app_path="$1"
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
budget="$(awk -F '=' '$1 ~ /^[[:space:]]*cold_launch_ms[[:space:]]*$/ {gsub(/[[:space:]]/, "", $2); print $2}' "$repo_root/perf/budgets.toml")"
if [[ ! "$budget" =~ ^[0-9]+([.][0-9]+)?$ ]]; then
  echo "Invalid cold_launch_ms budget" >&2
  exit 2
fi

median="$(perl -e '
use strict;
use warnings;
my $binary = shift @ARGV;
my @times;
for (1 .. 5) {
    my $pid = open(my $out, "-|");
    defined $pid or die "fork failed: $!\n";
    if ($pid == 0) { exec {$binary} $binary or die "exec failed: $!\n"; }
    my $ready;
    my $error;
    eval {
        local $SIG{ALRM} = sub { die "10 s launch watchdog expired\n" };
        alarm 10;
        while (my $line = <$out>) {
            if ($line =~ /^OP_READY ([0-9]+(?:\.[0-9]+)?)\s*$/) {
                $ready = $1;
                last;
            }
        }
        alarm 0;
    };
    $error = $@;
    alarm 0;
    kill "KILL", $pid;
    close $out;
    die $error if $error;
    defined $ready or die "app exited without OP_READY <ms>\n";
    push @times, $ready;
}
@times = sort { $a <=> $b } @times;
print "$times[2]\n";
 ' "$binary"
)"
echo "Cold launch median: $median ms over 5 runs; budget: $budget ms"
if awk -v measured="$median" -v limit="$budget" 'BEGIN {exit !(measured > limit)}'; then
  echo "FAIL launch time: median exceeds budget" >&2
  exit 1
fi
echo "PASS launch time"
