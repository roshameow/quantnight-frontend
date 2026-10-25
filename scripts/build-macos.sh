#!/bin/bash

# Local app build only: no install, launch, DMG, notarization or dependency install.
# Run from an exact source snapshot; release provenance belongs to the caller.
set -euo pipefail

fail() { printf 'build-macos: %s\n' "$*" >&2; exit 1; }
[[ $# -eq 0 ]] || fail "this entry accepts no build overrides"
[[ -z "${CARGO_BUILD_TARGET:-}" && -z "${TAURI_CONFIG:-}" ]] || \
  fail "target/config overrides are unsupported by this host app build"

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)"
cd "$ROOT"
# Make the output location explicit for both Cargo and Tauri. Relative caller
# values are resolved against this source root, never against Tauri's cwd.
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/src-tauri/target}"
case "$CARGO_TARGET_DIR" in
  /*) ;;
  *) CARGO_TARGET_DIR="$ROOT/$CARGO_TARGET_DIR" ;;
esac

# These names mirror tauri.conf.json productName and Cargo's binary name.
APP_DIR="$CARGO_TARGET_DIR/release/bundle/macos/QuantNight.app"
EXEC_PATH="$APP_DIR/Contents/MacOS/quantnight-frontend"
FRAMEWORKS_DIR="$APP_DIR/Contents/Frameworks"
LIB_NAME="libiconv.2.dylib"
BUNDLED_ID="@rpath/$LIB_NAME"
BUNDLE_RPATH='@executable_path/../Frameworks'
# Never patch an older bundle when a successful runner produces a different
# output layout. A repeat build must use an empty app destination chosen by owner.
[[ ! -e "$APP_DIR" && ! -L "$APP_DIR" ]] || fail "app destination already exists; use a fresh output directory"
for tool in npm brew otool install_name_tool codesign; do
  command -v "$tool" >/dev/null || fail "required local tool unavailable: $tool"
done

export CARGO_NET_OFFLINE=true
export npm_config_offline=true
export HOMEBREW_NO_AUTO_UPDATE=1
# This local route must not use inherited distribution/notarization credentials.
unset APPLE_ID APPLE_PASSWORD APPLE_TEAM_ID APPLE_API_KEY APPLE_API_ISSUER APPLE_API_KEY_PATH
export APPLE_SIGNING_IDENTITY=-
printf 'Building offline, locked, app-only through the original npm/Tauri route...\n'
npm run tauri -- build --bundles app -- --locked --offline
[[ -d "$APP_DIR" && ! -L "$APP_DIR" && -f "$EXEC_PATH" && ! -L "$EXEC_PATH" ]] || \
  fail "build did not produce the expected host app/executable"

# Capture command status before parsing; no process substitution may swallow an
# otool error. Strip the version suffix rather than splitting paths on spaces.
dependencies() {
  local listing
  listing=$(otool -L "$1") || return $?
  printf '%s\n' "$listing" | awk '/^[ \t]+.* \(compatibility version / {
    sub(/^[ \t]+/, ""); sub(/ \(compatibility version .*/, ""); print
  }'
}
rpaths() {
  local listing
  listing=$(otool -l "$1") || return $?
  printf '%s\n' "$listing" | awk '
    $1 == "cmd" { rpath = ($2 == "LC_RPATH") }
    rpath && $1 == "path" {
      sub(/^[ \t]*path /, ""); sub(/ \(offset [0-9]+\)$/, ""); print; rpath = 0
    }'
}
library_id() {
  local listing
  listing=$(otool -D "$1") || return $?
  printf '%s\n' "$listing" | awk 'NR > 1 && NF { print }'
}
replace_dependency() {
  # Literal line comparison works with spaces/glob characters and macOS Bash 3.2.
  local dep
  while IFS= read -r dep; do
    if [[ "$dep" == "$2" ]]; then printf '%s\n' "$3"; else printf '%s\n' "$dep"; fi
  done <<< "$1"
}

DEPS=$(dependencies "$EXEC_PATH")
ICONV=""
ICONV_COUNT=0
while IFS= read -r dep; do
  case "$dep" in
    */libiconv.2.dylib)
      ICONV="$dep"; ICONV_COUNT=$((ICONV_COUNT + 1)) ;;
    /usr/lib/*|/System/Library/*) ;;
    *) fail "unexpected or unreadable executable dependency" ;;
  esac
done <<< "$DEPS"
[[ $ICONV_COUNT -eq 1 ]] || fail "expected exactly one host libiconv dependency"

RPATHS=$(rpaths "$EXEC_PATH")
RPATH_COUNT=0
while IFS= read -r path; do
  [[ -n "$path" ]] || continue
  [[ "$path" == "$BUNDLE_RPATH" ]] || fail "unexpected executable RPATH; refusing ambiguous library resolution"
  RPATH_COUNT=$((RPATH_COUNT + 1))
done <<< "$RPATHS"
[[ $RPATH_COUNT -le 1 ]] || fail "duplicate bundle RPATH"

if [[ "$ICONV" != /usr/lib/libiconv.2.dylib ]]; then
  # Only the original Homebrew owner selects the library; never search Conda,
  # user configuration, arbitrary environment paths or a second package manager.
  PREFIX=$(brew --prefix libiconv)
  [[ "$PREFIX" == /* && "$PREFIX" != *$'\n'* ]] || fail "invalid Homebrew libiconv prefix"
  LIBICONV_SRC="$PREFIX/lib/$LIB_NAME"
  [[ -f "$LIBICONV_SRC" ]] || fail "Homebrew libiconv library is not installed"
  PHYSICAL_SRC="$(cd -- "$PREFIX/lib" && pwd -P)/$LIB_NAME"
  case "$ICONV" in
    "$BUNDLED_ID"|"$LIBICONV_SRC"|"$PHYSICAL_SRC") ;;
    *) fail "libiconv dependency is not the known Homebrew or bundled install name" ;;
  esac
  SOURCE_ID=$(library_id "$LIBICONV_SRC")
  case "$SOURCE_ID" in
    "$BUNDLED_ID"|"$LIBICONV_SRC"|"$PHYSICAL_SRC") ;;
    *) fail "unexpected Homebrew libiconv install name" ;;
  esac
  SOURCE_DEPS=$(dependencies "$LIBICONV_SRC")
  SELF_COUNT=0
  while IFS= read -r dep; do
    if [[ "$dep" == "$SOURCE_ID" ]]; then
      SELF_COUNT=$((SELF_COUNT + 1))
    else
      case "$dep" in
        /usr/lib/*|/System/Library/*) ;;
        *) fail "unexpected Homebrew libiconv transitive dependency" ;;
      esac
    fi
  done <<< "$SOURCE_DEPS"
  [[ $SELF_COUNT -eq 1 ]] || fail "unreadable or ambiguous Homebrew library identity"
  [[ ! -L "$FRAMEWORKS_DIR" && ! -e "$FRAMEWORKS_DIR/$LIB_NAME" && ! -L "$FRAMEWORKS_DIR/$LIB_NAME" ]] || \
    fail "unexpected existing bundled libiconv destination"
  mkdir -p "$FRAMEWORKS_DIR"
  cp "$LIBICONV_SRC" "$FRAMEWORKS_DIR/$LIB_NAME"
  install_name_tool -id "$BUNDLED_ID" "$FRAMEWORKS_DIR/$LIB_NAME"
  if [[ "$ICONV" != "$BUNDLED_ID" ]]; then
    install_name_tool -change "$ICONV" "$BUNDLED_ID" "$EXEC_PATH"
  fi
  if [[ $RPATH_COUNT -eq 0 ]]; then
    install_name_tool -add_rpath "$BUNDLE_RPATH" "$EXEC_PATH"
  fi
  # Prove the actual rewritten graph, not just a zero exit from install_name_tool.
  FINAL_DEPS=$(dependencies "$EXEC_PATH")
  [[ "$FINAL_DEPS" == "$(replace_dependency "$DEPS" "$ICONV" "$BUNDLED_ID")" ]] || fail "executable dependency rewrite verification failed"
  [[ "$(rpaths "$EXEC_PATH")" == "$BUNDLE_RPATH" ]] || fail "bundle RPATH verification failed"
  [[ "$(library_id "$FRAMEWORKS_DIR/$LIB_NAME")" == "$BUNDLED_ID" ]] || fail "bundled library ID verification failed"
  [[ "$(dependencies "$FRAMEWORKS_DIR/$LIB_NAME")" == "$(replace_dependency "$SOURCE_DEPS" "$SOURCE_ID" "$BUNDLED_ID")" ]] || \
    fail "bundled library dependency verification failed"
  codesign --force --sign - "$FRAMEWORKS_DIR/$LIB_NAME"
  codesign --verify --strict "$FRAMEWORKS_DIR/$LIB_NAME"
fi

# All Mach-O changes precede signing. Never use --deep as a signing shortcut;
# use it only to verify the finished app and its nested code/resources.
codesign --force --sign - "$APP_DIR"
codesign --verify --deep --strict "$APP_DIR"
printf 'Verified local ad-hoc app (not installed or launched): %s\n' "$APP_DIR"
