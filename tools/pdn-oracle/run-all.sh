#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"
export DOTNET_CLI_HOME="$PWD/.dotnet-home"
export NUGET_PACKAGES="$PWD/.nuget"
export DOTNET_CLI_TELEMETRY_OPTOUT=1
export DOTNET_SKIP_FIRST_TIME_EXPERIENCE=1
export DOTNET_NOLOGO=1
export PATH="$HOME/.dotnet:$PATH"
if [[ "${PDN_ORACLE_SKIP_FETCH:-0}" != 1 ]]; then
  bash tools/pdn-oracle/fetch-source.sh
fi
dotnet build tools/pdn-oracle/PdnOracle.csproj
for command in fixtures vectors blend resample effects-all flood; do
  dotnet run --no-build --project tools/pdn-oracle -- "$command"
done
python3 tests/oracle/legacy336/test_oracle.py
python3 -m unittest discover -s tests/oracle/legacy336 -p 'test_*.py'
