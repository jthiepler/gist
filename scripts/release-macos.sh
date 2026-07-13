#!/usr/bin/env bash
set -euo pipefail

# Release packaging invokes Perl-backed tools indirectly (notably `shasum` in
# the resource fingerprinting script). Use a locale available on every macOS
# installation so an invalid shell locale cannot flood release output.
export LC_ALL=C
export LANG=C

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RELEASE_DIR="$PROJECT_DIR/release"

# These are identifiers, not secrets. They can still be overridden when needed.
APPLE_ID="${APPLE_ID:-josh@jthiepler.com}"
APPLE_TEAM_ID="${APPLE_TEAM_ID:-UA47J69KU5}"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "This release command must run on macOS." >&2
  exit 1
fi

if [[ "$(uname -m)" != "arm64" ]]; then
  echo "Release builds must run on an Apple Silicon (arm64) Mac." >&2
  exit 1
fi

cd "$PROJECT_DIR"
echo "Checking synchronized application versions..."
npm run check-version

if [[ -z "${APPLE_SIGNING_IDENTITY:-}" ]]; then
  APPLE_SIGNING_IDENTITY="$(
    security find-identity -v -p codesigning |
      sed -n 's/.*"\(Developer ID Application:.*\)"/\1/p' |
      head -n 1
  )"
fi

if [[ -z "$APPLE_SIGNING_IDENTITY" ]]; then
  echo "No valid Developer ID Application identity was found in Keychain Access." >&2
  echo "Run: security find-identity -v -p codesigning" >&2
  exit 1
fi

if [[ -z "${TAURI_SIGNING_PRIVATE_KEY:-}" && -z "${TAURI_SIGNING_PRIVATE_KEY_PATH:-}" ]]; then
  printf 'Tauri updater private key: ' >&2
  read -r -s TAURI_SIGNING_PRIVATE_KEY
  printf '\n' >&2
  if [[ -z "$TAURI_SIGNING_PRIVATE_KEY" ]]; then
    echo "A Tauri updater signing key is required for release builds." >&2
    echo "Set TAURI_SIGNING_PRIVATE_KEY or TAURI_SIGNING_PRIVATE_KEY_PATH instead, or enter it at the prompt." >&2
    exit 1
  fi
  export TAURI_SIGNING_PRIVATE_KEY
fi

if [[ -z "${TAURI_SIGNING_PRIVATE_KEY_PASSWORD:-}" ]]; then
  printf 'Tauri updater private key password (leave blank if none): ' >&2
  read -r -s TAURI_SIGNING_PRIVATE_KEY_PASSWORD
  printf '\n' >&2
  export TAURI_SIGNING_PRIVATE_KEY_PASSWORD
fi

printf 'Apple app-specific password for %s: ' "$APPLE_ID" >&2
read -r -s APPLE_PASSWORD
printf '\n' >&2

if [[ -z "$APPLE_PASSWORD" ]]; then
  echo "An app-specific password is required." >&2
  exit 1
fi

# Keep the password only in this process and its build subprocesses.
trap 'unset APPLE_PASSWORD TAURI_SIGNING_PRIVATE_KEY TAURI_SIGNING_PRIVATE_KEY_PASSWORD' EXIT
export APPLE_ID APPLE_TEAM_ID APPLE_SIGNING_IDENTITY APPLE_PASSWORD

# Keep a clean, easy-to-find upload folder for each release. This directory is
# ignored by Git and only contains files intended for the GitHub Release.
rm -rf "$RELEASE_DIR"
mkdir -p "$RELEASE_DIR"

sign_resource_macho_files() {
  local resources_root="$PROJECT_DIR/src-tauri/resources/gist-sidecar"
  local signed_count=0

  echo "Signing Mach-O files inside bundled resources..."
  while IFS= read -r -d '' candidate; do
    if [[ "$(file -b "$candidate")" == *"Mach-O"* ]]; then
      codesign \
        --force \
        --options runtime \
        --timestamp \
        --sign "$APPLE_SIGNING_IDENTITY" \
        "$candidate"
      signed_count=$((signed_count + 1))
    fi
  done < <(find "$resources_root" -type f -print0)

  if (( signed_count == 0 )); then
    echo "No Mach-O files were found in bundled resources." >&2
    exit 1
  fi

  echo "Signed $signed_count bundled Mach-O files."
}

echo "Using signing identity: $APPLE_SIGNING_IDENTITY"
echo "Building the macOS sidecar and bundled resources..."
bash scripts/build-macos.sh --mode release
sign_resource_macho_files

echo "Building, signing, notarizing, and stapling the app and DMG..."
# The app target is required for Tauri to create the signed updater archive.
# Local package commands intentionally remove signing credentials, so the
# release path calls Tauri directly after preparing the DMG output directory.
bash scripts/prepare-dmg.sh
tauri build --bundles app,dmg

APP_PATH="$PROJECT_DIR/src-tauri/target/release/bundle/macos/Gist.app"
if [[ ! -d "$APP_PATH" ]]; then
  echo "Expected app bundle was not found: $APP_PATH" >&2
  exit 1
fi

echo "Verifying the signed app..."
codesign --verify --deep --strict --verbose=2 "$APP_PATH"
spctl --assess --type execute --verbose=4 "$APP_PATH"
xcrun stapler validate "$APP_PATH"

echo "Release complete. DMG files:"
find "$PROJECT_DIR/src-tauri/target/release/bundle/dmg" -maxdepth 1 -type f -name '*.dmg' -print
echo "Updater files to attach to the GitHub Release:"
find "$PROJECT_DIR/src-tauri/target/release/bundle" -maxdepth 3 -type f \( -name 'latest.json' -o -name '*.sig' -o -name '*.tar.gz' \) -print

DMG_DIR="$PROJECT_DIR/src-tauri/target/release/bundle/dmg"
BUNDLE_DIR="$PROJECT_DIR/src-tauri/target/release/bundle"
DMG_FILE="$(find "$DMG_DIR" -maxdepth 1 -type f -name '*.dmg' -print -quit)"
UPDATER_ARCHIVE="$BUNDLE_DIR/macos/Gist.app.tar.gz"
UPDATER_SIGNATURE="$UPDATER_ARCHIVE.sig"

if [[ -z "$DMG_FILE" || ! -f "$UPDATER_ARCHIVE" || ! -f "$UPDATER_SIGNATURE" ]]; then
  echo "Release artifacts are incomplete; cannot populate $RELEASE_DIR." >&2
  echo "Expected a DMG, $UPDATER_ARCHIVE, and $UPDATER_SIGNATURE." >&2
  exit 1
fi

cp -p "$DMG_FILE" "$RELEASE_DIR/"
cp -p "$UPDATER_ARCHIVE" "$RELEASE_DIR/"
cp -p "$UPDATER_SIGNATURE" "$RELEASE_DIR/"

APP_VERSION="$(sed -n 's/.*"version": "\([^"]*\)".*/\1/p' "$PROJECT_DIR/src-tauri/tauri.conf.json" | head -n 1)"
SIGNATURE="$(tr -d '\r\n' < "$UPDATER_SIGNATURE")"
PUB_DATE="$(date -u '+%Y-%m-%dT%H:%M:%SZ')"
printf '{\n  "version": "%s",\n  "notes": "See the GitHub Release notes for this version.",\n  "pub_date": "%s",\n  "platforms": {\n    "darwin-aarch64": {\n      "signature": "%s",\n      "url": "https://github.com/jthiepler/gist/releases/latest/download/Gist.app.tar.gz"\n    }\n  }\n}\n' \
  "$APP_VERSION" "$PUB_DATE" "$SIGNATURE" > "$RELEASE_DIR/latest.json"

echo "GitHub Release upload folder: $RELEASE_DIR"
find "$RELEASE_DIR" -maxdepth 1 -type f -print
