#!/usr/bin/env bash
set -euo pipefail

# Keep local packaging quiet and deterministic when the invoking shell exports
# a locale that is not installed on the machine (for example, en_FR.UTF-8).
export LC_ALL=C
export LANG=C

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [[ "${1:-}" == "build" ]]; then
  "$SCRIPT_DIR/build-macos.sh" --mode development
  "$SCRIPT_DIR/prepare-dmg.sh"

  # Local app/DMG builds intentionally skip updater signing, Developer ID
  # signing, and notarization. The release script owns that workflow.
  rm -f \
    "$SCRIPT_DIR/../src-tauri/target/release/bundle/macos/Gist.app.tar.gz" \
    "$SCRIPT_DIR/../src-tauri/target/release/bundle/macos/Gist.app.tar.gz.sig"
  DEV_CONFIG='{"bundle":{"createUpdaterArtifacts":false}}'
  exec env \
    -u APPLE_SIGNING_IDENTITY \
    -u APPLE_ID \
    -u APPLE_PASSWORD \
    -u APPLE_TEAM_ID \
    -u APPLE_API_ISSUER \
    -u APPLE_API_KEY \
    -u APPLE_API_KEY_PATH \
    -u TAURI_SIGNING_PRIVATE_KEY \
    -u TAURI_SIGNING_PRIVATE_KEY_PATH \
    -u TAURI_SIGNING_PRIVATE_KEY_PASSWORD \
    GIST_DEVELOPER_FEATURES=1 \
    tauri "$@" --config "$DEV_CONFIG"
fi

exec tauri "$@"
