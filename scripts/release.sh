#!/usr/bin/env bash
# M60: builds a signed, notarized OpenPainter DMG and its signed Sparkle appcast.
#
# Usage: scripts/release.sh VERSION [--dry-run] [--allow-dirty]
#        make -C mac release VERSION=1.0.0
#
#   VERSION        semver (1.0.0, 1.0.0-rc.1), higher than the highest v* tag.
#   --dry-run      everything except Apple's notary service, stapling and the
#                  Gatekeeper checks that need a notarized app.
#   --allow-dirty  build from a working tree with local changes. Only for a
#                  prerelease VERSION (one with a "-"); a real release always
#                  comes from a committed tree.
#
# Output (build/release, ignored by git):
#   OpenPainter-VERSION.dmg   the notarized, stapled DMG (app plus Applications link)
#   appcast.xml               the signed Sparkle feed for this version
#   OpenPainter.app           the stapled app the DMG holds
#   check/                    the local feed copy the Sparkle check used
#
# Version numbers: CFBundleShortVersionString = VERSION. CFBundleVersion (and
# sparkle:version) = the number of commits on HEAD (`git rev-list --count
# HEAD`). The highest v* tag must be an ancestor of HEAD and a different
# commit, so the build number always goes up (Sparkle refuses a lower
# CFBundleVersion: docs/ARCHITECTURE.md "Releases and updates").
#
# Signing (https://sparkle-project.org/documentation/sandboxing/, "Code
# Signing"): Xcode's Code Sign on Copy does not re-sign Sparkle's XPC services
# and helpers, so this script re-signs Installer.xpc, Downloader.xpc (keeping
# its entitlements), Autoupdate, Updater.app, Sparkle.framework, every other
# framework and the app, inside out, with the Developer ID, hardened runtime
# and a secure timestamp. No --deep signing.
#
# Appcast (https://sparkle-project.org/documentation/publishing/): sign_update
# signs the DMG (sparkle:edSignature plus length) and then the appcast itself
# (SURequireSignedFeed). The private key comes from
# OPENPAINTER_SPARKLE_PRIVATE_KEY in ~/keys: sourced in a subshell, written to
# a 0600 file in a private temporary folder outside the repo, handed to
# sign_update with -f, and deleted as soon as signing ends (and on any exit).
# It is never printed.
#
# Keychain: the first codesign with a new key shows "codesign wants to sign
# using key". Click Always Allow; the script waits for it.
set -Eeuo pipefail

step='input checks'
trap 'result=$?; printf "FAIL: %s (exit %s)\n" "$step" "$result" >&2' ERR
pass() { printf 'PASS: %s\n' "$step"; }
fail() { printf 'FAIL: %s: %s\n' "$step" "$1" >&2; exit 1; }

version=''
dry_run=false
allow_dirty=false
for argument in "$@"; do
  case "$argument" in
    --dry-run) dry_run=true ;;
    --allow-dirty) allow_dirty=true ;;
    -*) fail "unknown option: $argument" ;;
    *)
      [[ -z "$version" ]] || fail 'usage: scripts/release.sh VERSION [--dry-run] [--allow-dirty]'
      version="$argument"
      ;;
  esac
done
[[ -n "$version" ]] || fail 'usage: scripts/release.sh VERSION [--dry-run] [--allow-dirty]'
# Semantic Versioning 2.0.0, without build metadata.
semver_re='^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-((0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)(\.(0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*))*))?$'
[[ "$version" =~ $semver_re ]] || fail "VERSION must be semver like 1.0.0 or 1.0.0-rc.1, got: $version"
if "$allow_dirty" && [[ "$version" != *-* ]]; then
  fail '--allow-dirty is only for a prerelease VERSION (like 1.0.0-rc.1)'
fi
pass

step='tool checks'
for tool in git xcodegen xcodebuild codesign ditto hdiutil xcrun spctl shasum python3 file find sort security openssl swift; do
  command -v "$tool" >/dev/null 2>&1 || fail "missing tool: $tool"
done
[[ -x /usr/libexec/PlistBuddy ]] || fail 'missing tool: /usr/libexec/PlistBuddy'
pass

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
mac="$repo_root/mac"
cd "$repo_root"

step='git checks'
if [[ -n "$(git status --porcelain)" ]]; then
  if "$allow_dirty"; then
    printf 'WARNING: building %s from a working tree with local changes (--allow-dirty)\n' "$version" >&2
  else
    fail 'git tree is not clean; commit or remove local changes before releasing'
  fi
fi
if git rev-parse -q --verify "refs/tags/v$version" >/dev/null; then
  fail "tag v$version already exists"
fi
# Highest v* tag by semver precedence (prereleases sort below their release).
semver_max() {
  python3 -c '
import re, sys
pat = re.compile(r"^v(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?$")
def key(tag):
    m = pat.match(tag)
    core = tuple(int(x) for x in m.group(1, 2, 3))
    pre = m.group(4)
    if pre is None:
        return (core, 1, ())
    parts = tuple((0, int(p), "") if p.isdigit() else (1, 0, p) for p in pre.split("."))
    return (core, 0, parts)
tags = [t for t in sys.stdin.read().split() if pat.match(t)]
if tags:
    print(max(tags, key=key))
'
}
semver_greater() { # semver_greater A B: true when A > B
  python3 -c '
import re, sys
pat = re.compile(r"^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?$")
def key(v):
    m = pat.match(v)
    core = tuple(int(x) for x in m.group(1, 2, 3))
    pre = m.group(4)
    if pre is None:
        return (core, 1, ())
    return (core, 0, tuple((0, int(p), "") if p.isdigit() else (1, 0, p) for p in pre.split(".")))
sys.exit(0 if key(sys.argv[1]) > key(sys.argv[2]) else 1)
' "$1" "$2"
}
last_tag="$(git tag --list 'v*' | semver_max)"
build_number="$(git rev-list --count HEAD)"
[[ "$build_number" =~ ^[0-9]+$ && "$build_number" -gt 1 ]] || fail 'cannot read the git commit count'
if [[ -n "$last_tag" ]]; then
  semver_greater "$version" "${last_tag#v}" || fail "VERSION $version is not higher than the last tag $last_tag"
  git merge-base --is-ancestor "$last_tag" HEAD || fail "$last_tag is not an ancestor of HEAD, so the build number may not go up"
  last_build="$(git rev-list --count "$last_tag")"
  [[ "$build_number" -gt "$last_build" ]] || fail "build number $build_number is not higher than $last_tag's $last_build"
  printf 'Last tag %s (build %s)\n' "$last_tag" "$last_build"
else
  printf 'No v* tag yet\n'
fi
printf 'VERSION %s, build number %s (commits on HEAD %s)\n' "$version" "$build_number" "$(git rev-parse --short HEAD)"
pass

signing_identity='Developer ID Application: Yi Li (8Q5V8XK85Y)'
team_id='8Q5V8XK85Y'
public_key='Q1P9GEU6X00XfYERTrMaaVhfra04UVYC2IuWAK0TJoc='
profile="${NOTARY_PROFILE:-smh-notary}"
dmg_name="OpenPainter-$version.dmg"
download_url="https://github.com/wizdes/OpenPainter/releases/download/v$version/$dmg_name"
release_page="https://github.com/wizdes/OpenPainter/releases/tag/v$version"

step='Developer ID identity'
identities="$(security find-identity -v -p codesigning)"
identity_count="$(grep -cF "\"$signing_identity\"" <<< "$identities" || true)"
[[ "$identity_count" == 1 ]] || fail "expected exactly one '$signing_identity' identity in the keychain, found $identity_count"
identity_hash="$(grep -F "\"$signing_identity\"" <<< "$identities" | awk '{print $2}')"
cert_end="$(security find-certificate -a -Z -p -c "$signing_identity" | python3 -c '
import sys, subprocess
want = sys.argv[1]
text = sys.stdin.read()
blocks = text.split("SHA-1 hash: ")[1:]
for b in blocks:
    sha1 = b.split()[0]
    if sha1 != want:
        continue
    pem = b[b.index("-----BEGIN CERTIFICATE-----"): b.index("-----END CERTIFICATE-----") + 25]
    out = subprocess.run(["openssl", "x509", "-noout", "-enddate"], input=pem.encode(), capture_output=True, check=True)
    print(out.stdout.decode().strip().split("=", 1)[1])
    break
' "$identity_hash")"
[[ -n "$cert_end" ]] || fail 'cannot read the certificate end date'
days_left="$(python3 -c 'import sys, datetime as d; e=d.datetime.strptime(sys.argv[1], "%b %d %H:%M:%S %Y %Z"); print((e-d.datetime.utcnow()).days)' "$cert_end")"
printf 'Identity SHA-1 %s..., certificate ends %s (%s days)\n' "${identity_hash:0:8}" "$cert_end" "$days_left"
[[ "$days_left" -ge 30 ]] || fail "the Developer ID certificate ends in $days_left days; renew it first"
pass

if ! "$dry_run"; then
  step='notary profile check'
  if ! xcrun notarytool history --keychain-profile "$profile" --output-format json >/dev/null; then
    fail "notary profile '$profile' is unavailable. Store it with: xcrun notarytool store-credentials $profile --apple-id <email> --team-id $team_id"
  fi
  pass
fi

# The private key lives only in a 0600 file inside a 0700 folder outside the
# repo, removed at exit. Everything the EXIT trap cleans up is declared here.
key_dir=''
key_file=''
mounted_device=''
server_pid=''
cleanup() {
  if [[ -n "$key_dir" && -d "$key_dir" ]]; then rm -rf -- "$key_dir"; fi
  if [[ -n "$mounted_device" ]]; then hdiutil detach -force "$mounted_device" >/dev/null 2>&1 || true; fi
  if [[ -n "$server_pid" ]]; then kill "$server_pid" 2>/dev/null || true; fi
}
trap cleanup EXIT
trap 'exit 130' INT TERM

step='release key'
key_dir="$(mktemp -d "${TMPDIR:-/tmp}/op-release-key.XXXXXX")"
chmod 700 "$key_dir"
key_file="$key_dir/ed25519"
(umask 077; : > "$key_file")
chmod 600 "$key_file"
[[ -f "$HOME/keys" ]] || fail "$HOME/keys is missing"
# shellcheck disable=SC1091
( set +x; source "$HOME/keys" >/dev/null 2>&1; printf '%s' "${OPENPAINTER_SPARKLE_PRIVATE_KEY:-}" ) > "$key_file"
[[ -s "$key_file" ]] || fail 'OPENPAINTER_SPARKLE_PRIVATE_KEY is missing from ~/keys'
derived_key="$(swift "$repo_root/scripts/sparkle-public-key.swift" < "$key_file")" || fail 'cannot read the private key'
[[ "$derived_key" == "$public_key" ]] || fail "the private key in ~/keys does not match SUPublicEDKey $public_key"
pass

step='SUPublicEDKey in project.yml'
grep -qF "SUPublicEDKey: \"$public_key\"" "$mac/project.yml" || fail 'mac/project.yml does not ship the release public key'
pass

release_dir="$repo_root/build/release"
derived_data="$release_dir/DerivedData"
built_app="$derived_data/Build/Products/Release/OpenPainter.app"
app="$release_dir/OpenPainter.app"
dmg="$release_dir/$dmg_name"
check_dir="$release_dir/check"

step='clean release folder'
[[ -n "$repo_root" && "$repo_root" != / ]] || fail "bad repo root: $repo_root"
[[ "$release_dir" == "$repo_root/build/release" ]] || fail "refusing to delete unexpected path: $release_dir"
if [[ -e "$release_dir" ]]; then rm -rf -- "$release_dir"; fi
[[ ! -e "$release_dir" ]] || fail "could not delete $release_dir"
mkdir -p "$release_dir" "$check_dir"
pass

step='build engine (fat LTO)'
"$mac/scripts/build-engine.sh"
[[ -s "$repo_root/engine/target/app-release/libop_ffi.a" ]] || fail 'libop_ffi.a missing'
pass

step='generate Xcode project'
(cd "$mac" && xcodegen generate --spec project.yml --quiet)
resolved="$mac/OpenPainter.xcodeproj/project.xcworkspace/xcshareddata/swiftpm/Package.resolved"
mkdir -p "$(dirname "$resolved")"
cp "$mac/Package.resolved" "$resolved"
if ! "$allow_dirty" && [[ -n "$(git status --porcelain)" ]]; then
  git status --porcelain >&2
  fail 'xcodegen changed tracked files (mac/OpenPainter/Info.plist?); regenerate and commit first'
fi
pass

step='build Release app'
xcodebuild -project "$mac/OpenPainter.xcodeproj" -scheme OpenPainter -configuration Release \
  -derivedDataPath "$derived_data" -onlyUsePackageVersionsFromResolvedFile \
  MARKETING_VERSION="$version" CURRENT_PROJECT_VERSION="$build_number" \
  CODE_SIGN_INJECT_BASE_ENTITLEMENTS=NO \
  build > "$release_dir/xcodebuild.log" 2>&1 || { tail -40 "$release_dir/xcodebuild.log" >&2; fail "xcodebuild failed; log: $release_dir/xcodebuild.log"; }
[[ -d "$built_app" ]] || fail "built app missing: $built_app"
ditto "$built_app" "$app"
info="$app/Contents/Info.plist"
plist() { /usr/libexec/PlistBuddy -c "Print :$1" "$info"; }
[[ "$(plist CFBundleShortVersionString)" == "$version" ]] || fail 'CFBundleShortVersionString does not match VERSION'
[[ "$(plist CFBundleVersion)" == "$build_number" ]] || fail 'CFBundleVersion does not match the build number'
[[ "$(plist SUPublicEDKey)" == "$public_key" ]] || fail 'SUPublicEDKey is not the release key'
[[ "$(plist SUEnableAutomaticChecks)" == true ]] || fail 'SUEnableAutomaticChecks is not on'
[[ "$(plist SURequireSignedFeed)" == true ]] || fail 'SURequireSignedFeed is not on'
[[ "$(plist SUVerifyUpdateBeforeExtraction)" == true ]] || fail 'SUVerifyUpdateBeforeExtraction is not on'
[[ "$(plist LSMinimumSystemVersion)" == 15.0 ]] || fail 'LSMinimumSystemVersion is not 15.0'
[[ "$(plist CFBundleIdentifier)" == dev.yili.OpenPainter ]] || fail 'wrong bundle identifier'
[[ "$(plist CFBundleIconName)" == AppIcon && -f "$app/Contents/Resources/AppIcon.icns" ]] || fail 'the app icon is missing (AppIcon in Assets.xcassets)'
printf 'Built %s %s (%s), %s\n' "$(plist CFBundleIdentifier)" "$version" "$build_number" "$(du -sh "$app" | cut -f1)"
pass

# Every Mach-O file under the app, one per line.
list_macho() {
  local path
  find "$app" -type f -print0 | while IFS= read -r -d '' path; do
    if [[ "$(file -b -- "$path")" == *'Mach-O'* ]]; then printf '%s\n' "$path"; fi
  done | sort
}
sign() { codesign --force --timestamp --options runtime --sign "$signing_identity" "$@"; }

step='sign with Developer ID (inside out)'
printf 'If a Keychain dialog asks to let codesign use the key, click Always Allow.\n'
sparkle="$app/Contents/Frameworks/Sparkle.framework"
sparkle_b="$sparkle/Versions/B"
[[ -d "$sparkle_b" ]] || fail "Sparkle.framework/Versions/B missing in $app"
for part in "$sparkle_b/XPCServices/Installer.xpc" "$sparkle_b/Autoupdate" "$sparkle_b/Updater.app"; do
  [[ -e "$part" ]] || fail "missing Sparkle part: $part"
  sign -- "$part"
done
[[ -e "$sparkle_b/XPCServices/Downloader.xpc" ]] || fail 'missing Sparkle part: Downloader.xpc'
sign --preserve-metadata=entitlements -- "$sparkle_b/XPCServices/Downloader.xpc"
sign -- "$sparkle"
# Other frameworks and loose dylibs (OpenPainterKit and anything Xcode embedded).
while IFS= read -r -d '' framework; do
  [[ "$framework" == "$sparkle" ]] && continue
  sign -- "$framework"
done < <(find "$app/Contents/Frameworks" -maxdepth 1 -name '*.framework' -print0 2>/dev/null)
while IFS= read -r -d '' dylib; do
  sign -- "$dylib"
done < <(find "$app/Contents" -name '*.dylib' -not -path '*.framework/*' -print0)
# The app last. It has no entitlements (no sandbox, no exceptions).
sign -- "$app"
pass

verify_signatures() {
  local target_app="$1" bad=false path details path_entitlements
  codesign --verify --deep --strict --verbose=2 "$target_app"
  while IFS= read -r path; do
    if ! details="$(codesign -dvv -- "$path" 2>&1)"; then
      printf 'BAD SIGNATURE: %s: not signed\n' "$path" >&2; bad=true; continue
    fi
    if ! path_entitlements="$(codesign -d --entitlements - -- "$path" 2>&1)"; then
      printf 'BAD SIGNATURE: %s: cannot read entitlements\n' "$path" >&2; bad=true
    elif [[ "$path_entitlements" == *'com.apple.security.get-task-allow'* ]]; then
      printf 'BAD SIGNATURE: %s: has com.apple.security.get-task-allow\n' "$path" >&2; bad=true
    fi
    [[ "$details" == *$'\n''Timestamp='* ]] || { printf 'BAD SIGNATURE: %s: no secure timestamp\n' "$path" >&2; bad=true; }
    [[ "$details" == *'flags='*'(runtime)'* ]] || { printf 'BAD SIGNATURE: %s: no hardened runtime\n' "$path" >&2; bad=true; }
    [[ "$details" == *$'\n'"Authority=$signing_identity"$'\n'* ]] || { printf 'BAD SIGNATURE: %s: authority is not %s\n' "$path" "$signing_identity" >&2; bad=true; }
    [[ "$details" == *$'\n'"TeamIdentifier=$team_id"$'\n'* ]] || { printf 'BAD SIGNATURE: %s: team is not %s\n' "$path" "$team_id" >&2; bad=true; }
    printf 'OK: %s\n' "${path#"$target_app/"}"
  done < <(app="$target_app" list_macho)
  [[ "$bad" == false ]] || return 1
  [[ "$(codesign -d --entitlements - "$target_app" 2>&1)" != *'com.apple.security.app-sandbox'* ]] || { printf 'app has a sandbox entitlement\n' >&2; return 1; }
}

step='check every Mach-O'
macho_count="$(list_macho | wc -l | tr -d ' ')"
[[ "$macho_count" -gt 0 ]] || fail 'no Mach-O files found in the app'
verify_signatures "$app" || fail 'Mach-O signature checks failed'
printf '%s Mach-O files checked\n' "$macho_count"
pass

step='no Homebrew or local library paths'
while IFS= read -r path; do
  # Dependency lines start with a tab; the others name the file and architecture.
  if otool -L -arch all "$path" | grep -E $'^\t' | grep -E '/opt/homebrew|/usr/local|/Users/' >&2; then
    fail "$path links a library outside the system and the app"
  fi
done < <(list_macho)
pass

notary_ids=()
submit_for_notarization() {
  local artifact="$1" submission submission_id status
  if ! submission="$(xcrun notarytool submit "$artifact" --keychain-profile "$profile" --wait --output-format json)"; then
    printf '%s\n' "$submission" >&2
    submission_id="$(printf '%s' "$submission" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("id", ""))' 2>/dev/null || true)"
    if [[ -n "$submission_id" ]]; then xcrun notarytool log "$submission_id" --keychain-profile "$profile" || true; fi
    fail 'notarytool submit failed'
  fi
  submission_id="$(printf '%s' "$submission" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("id", ""))')"
  status="$(printf '%s' "$submission" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("status", ""))')"
  printf 'Notary submission %s: %s\n' "$submission_id" "$status"
  notary_ids+=("$(basename "$artifact"): $submission_id $status")
  if [[ "$status" != Accepted ]]; then
    if [[ -n "$submission_id" ]]; then xcrun notarytool log "$submission_id" --keychain-profile "$profile" || true; fi
    fail "notary status: ${status:-unknown}"
  fi
  xcrun notarytool log "$submission_id" --keychain-profile "$profile" "$release_dir/notary-$(basename "$artifact").json" >/dev/null
}

if ! "$dry_run"; then
  step='notarize app'
  mkdir -p "$release_dir/notary"
  notary_zip="$release_dir/notary/OpenPainter-$version.zip"
  ditto -c -k --keepParent "$app" "$notary_zip"
  submit_for_notarization "$notary_zip"
  rm "$notary_zip"
  rmdir "$release_dir/notary"
  pass

  step='staple app'
  xcrun stapler staple "$app"
  xcrun stapler validate "$app"
  spctl --assess --type execute -vv "$app"
  pass
else
  printf 'SKIP: notarize and staple the app (--dry-run)\n'
fi

step='create DMG'
staging="$release_dir/dmg-staging"
mkdir -p "$staging"
ditto "$app" "$staging/OpenPainter.app"
ln -s /Applications "$staging/Applications"
hdiutil create -volname "OpenPainter $version" -srcfolder "$staging" -ov -format UDZO "$dmg"
[[ -s "$dmg" ]] || fail "DMG missing: $dmg"
rm -rf -- "$staging"
pass

step='sign DMG'
codesign --force --timestamp --sign "$signing_identity" "$dmg"
codesign --verify --strict --verbose=2 "$dmg"
pass

if ! "$dry_run"; then
  step='notarize DMG'
  submit_for_notarization "$dmg"
  pass

  step='staple DMG'
  xcrun stapler staple "$dmg"
  xcrun stapler validate "$dmg"
  spctl --assess --type open --context context:primary-signature -vv "$dmg"
  pass
else
  printf 'SKIP: notarize and staple the DMG (--dry-run)\n'
fi

step='verify DMG contents (mounted read-only)'
mount_dir="$(mktemp -d "$release_dir/dmg-mount.XXXXXX")"
attach_plist="$release_dir/attach.plist"
hdiutil attach -readonly -nobrowse -noautoopen -plist -mountpoint "$mount_dir" "$dmg" > "$attach_plist"
mounted_device="$(python3 -c '
import plistlib, sys
entities = plistlib.load(open(sys.argv[1], "rb"))["system-entities"]
print(min((e["dev-entry"] for e in entities), key=len))
' "$attach_plist")"
printf 'Mounted %s at %s\n' "$mounted_device" "$mount_dir"
verify_mounted() {
  local mounted_app="$mount_dir/OpenPainter.app"
  [[ -d "$mounted_app" ]] || { printf 'missing OpenPainter.app in DMG\n' >&2; return 1; }
  [[ -L "$mount_dir/Applications" && "$(readlink "$mount_dir/Applications")" == /Applications ]] || { printf 'missing Applications link\n' >&2; return 1; }
  [[ "$(find "$mount_dir" -mindepth 1 -maxdepth 1 -not -name '.*' | wc -l | tr -d ' ')" == 2 ]] || { printf 'unexpected items in DMG\n' >&2; ls -la "$mount_dir" >&2; return 1; }
  [[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleVersion' "$mounted_app/Contents/Info.plist")" == "$build_number" ]] || { printf 'wrong build in DMG\n' >&2; return 1; }
  codesign --verify --deep --strict --verbose=2 "$mounted_app" || return 1
  if ! "$dry_run"; then
    xcrun stapler validate "$mounted_app" || return 1
    spctl -a -vv "$mounted_app" || return 1
  fi
}
verified=true
verify_mounted || verified=false
hdiutil detach "$mounted_device" || fail "could not detach $mounted_device"
mounted_device=''
rmdir "$mount_dir"
rm "$attach_plist"
[[ "$verified" == true ]] || fail 'DMG contents failed verification'
pass

sign_update="$derived_data/SourcePackages/artifacts/sparkle/Sparkle/bin/sign_update"
step='Sparkle sign_update'
[[ -x "$sign_update" ]] || fail "sign_update missing: $sign_update"
pass

dmg_length="$(stat -f %z "$dmg")"
write_appcast() { # write_appcast OUT URL SIGNATURE
  python3 - "$1" "$2" "$3" "$version" "$build_number" "$dmg_length" "$release_page" <<'PY'
import sys
from email.utils import formatdate
from xml.sax.saxutils import escape, quoteattr
out, url, sig, version, build, length, page = sys.argv[1:]
xml = f'''<?xml version="1.0" encoding="utf-8"?>
<rss version="2.0" xmlns:sparkle="http://www.andymatuschak.org/xml-namespaces/sparkle">
  <channel>
    <title>OpenPainter</title>
    <link>https://yili.dev/projects/openpainter/</link>
    <description>OpenPainter updates</description>
    <language>en</language>
    <item>
      <title>{escape("Version " + version)}</title>
      <link>{escape(page)}</link>
      <sparkle:version>{escape(build)}</sparkle:version>
      <sparkle:shortVersionString>{escape(version)}</sparkle:shortVersionString>
      <sparkle:minimumSystemVersion>15.0.0</sparkle:minimumSystemVersion>
      <sparkle:hardwareRequirements>arm64</sparkle:hardwareRequirements>
      <pubDate>{formatdate(usegmt=True)}</pubDate>
      <enclosure url={quoteattr(url)} length={quoteattr(length)}
                 type="application/octet-stream" sparkle:edSignature={quoteattr(sig)}/>
    </item>
  </channel>
</rss>
'''
open(out, "w", encoding="utf-8").write(xml)
PY
}

step='sign DMG for Sparkle and write appcast.xml'
ed_signature="$("$sign_update" -f "$key_file" -p "$dmg")"
[[ "$(printf '%s' "$ed_signature" | base64 -d 2>/dev/null | wc -c | tr -d ' ')" == 64 ]] || fail 'sign_update did not print a 64-byte EdDSA signature'
write_appcast "$release_dir/appcast.xml" "$download_url" "$ed_signature"
"$sign_update" -f "$key_file" --disable-signing-warning "$release_dir/appcast.xml"
grep -q '<!-- sparkle-signatures:' "$release_dir/appcast.xml" || fail 'appcast.xml has no feed signature'
pass

step='local feed copy for the Sparkle check'
local_port="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1]); s.close()')"
write_appcast "$check_dir/appcast-local.xml" "http://127.0.0.1:$local_port/$dmg_name" "$ed_signature"
"$sign_update" -f "$key_file" --disable-signing-warning "$check_dir/appcast-local.xml"
grep -q '<!-- sparkle-signatures:' "$check_dir/appcast-local.xml" || fail 'appcast-local.xml has no feed signature'
pass

step='remove the private key copy'
rm -rf -- "$key_dir"
key_dir=''
key_file=''
pass

step='Sparkle accepts the feed and the DMG (UpdateFeedTests, real public key)'
test_log="$check_dir/feed-check.log"
tests='OpenPainterKitTests/UpdateFeedTests'
if ! (cd "$mac" && env \
  TEST_RUNNER_OP_RELEASE_DIR="$release_dir" \
  TEST_RUNNER_OP_RELEASE_VERSION="$version" \
  TEST_RUNNER_OP_RELEASE_BUILD="$build_number" \
  TEST_RUNNER_OP_RELEASE_DMG_URL="$download_url" \
  TEST_RUNNER_OP_RELEASE_LOCAL_PORT="$local_port" \
  xcodebuild -project OpenPainter.xcodeproj -scheme OpenPainter -derivedDataPath build/DerivedData \
    -onlyUsePackageVersionsFromResolvedFile -configuration Debug test \
    "-only-testing:$tests/testReleaseFeedIsAcceptedWithTheReleaseKey" \
    "-only-testing:$tests/testReleaseDMGIsValidatedAndInstalledBySparkle" \
    "-only-testing:$tests/testAppInfoPlistHasTheFeedAndTheReleaseKey" > "$test_log" 2>&1); then
  grep -E 'error:|failed|Test Case .* failed' "$test_log" | tail -30 >&2 || true
  fail "the Sparkle feed check failed; log: $test_log"
fi
grep -E '^Test Case .*(passed|failed|skipped)|M60 release' "$test_log" | sort -u
grep -q 'Executed 3 tests, with 0 failures' "$test_log" || fail 'expected 3 executed tests with 0 failures'
! grep -qE "^Test Case '.*' skipped|Executed [0-9]+ tests?, with [1-9][0-9]* tests? skipped" "$test_log" || fail 'a release check test was skipped'
pass

step='no zip left'
[[ -z "$(find "$release_dir" -maxdepth 1 -name '*.zip' -print -quit)" ]] || fail 'a zip remains in build/release'
pass

step='summary'
printf '\nRelease %s (build %s)%s\n' "$version" "$build_number" "$("$dry_run" && echo ' DRY RUN: not notarized')"
printf '  DMG:      %s\n' "$dmg"
printf '  appcast:  %s\n' "$release_dir/appcast.xml"
printf '  download: %s\n' "$download_url"
for entry in "${notary_ids[@]+"${notary_ids[@]}"}"; do printf '  notary:   %s\n' "$entry"; done
printf '  SHA-256:  '
shasum -a 256 "$dmg"
pass
printf 'Next steps (after review): git tag v%s; gh release create v%s %q; publish appcast.xml to the SUFeedURL.\n' "$version" "$version" "$dmg"
