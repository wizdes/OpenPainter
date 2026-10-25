#!/bin/bash
# Downloads the Camera RAW samples listed in tools/raw-samples.txt into
# ~/Library/Caches/OpenPainter/raw-samples (outside the repo) and checks each
# SHA-256. The M17 RAW import tests skip, with a printed reason, when a file is
# missing. Files already present with the right checksum are not downloaded again.
set -euo pipefail

list="$(cd "$(dirname "$0")" && pwd)/raw-samples.txt"
dest="${OPENPAINTER_RAW_SAMPLES:-$HOME/Library/Caches/OpenPainter/raw-samples}"
mkdir -p "$dest"

failed=0
while IFS=$'\t' read -r name sha license maker model url; do
    [[ -z "$name" || "$name" == \#* ]] && continue
    out="$dest/$name"
    if [[ -f "$out" ]] && [[ "$(shasum -a 256 "$out" | cut -d' ' -f1)" == "$sha" ]]; then
        echo "ok      $name ($maker $model, $license) already present"
        continue
    fi
    tmp="$out.part"
    rm -f "$tmp"
    echo "fetch   $name from $url"
    if ! curl --fail --silent --show-error --location --max-time 300 --output "$tmp" "$url"; then
        echo "FAILED  $name: download failed" >&2
        rm -f "$tmp"
        failed=1
        continue
    fi
    got="$(shasum -a 256 "$tmp" | cut -d' ' -f1)"
    if [[ "$got" != "$sha" ]]; then
        echo "FAILED  $name: SHA-256 is $got, expected $sha" >&2
        rm -f "$tmp"
        failed=1
        continue
    fi
    mv "$tmp" "$out"
    echo "ok      $name ($maker $model, $license)"
done < "$list"

if [[ $failed -ne 0 ]]; then
    echo "Some samples are missing; the RAW tests will skip them." >&2
    exit 1
fi
echo "All samples are in $dest"
