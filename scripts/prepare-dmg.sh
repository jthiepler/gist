#!/usr/bin/env bash
set -euo pipefail

# The Tauri DMG bundler uses the product name as the mounted volume name.
# A failed or interrupted build can leave an older generated Gist DMG mounted,
# causing the next hdiutil attach step to fail because /Volumes/Gist is busy.

if [[ "$(uname -s)" != "Darwin" ]]; then
  exit 0
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
DMG_DIR="$PROJECT_DIR/src-tauri/target/release/bundle/dmg"

mounted_gist_images() {
  hdiutil info 2>/dev/null | awk '
    /^image-path[[:space:]]*:/ {
      image = $0
      sub(/^[^:]*:[[:space:]]*/, "", image)
      next
    }
    /^\/dev\/disk/ {
      device = $1
      next
    }
    /[[:space:]]\/Volumes\/Gist$/ {
      if (image ~ /(^|\/)Gist_[^\/]+\.dmg$/ && device != "") {
        print device "\t" image
      }
      device = ""
      image = ""
    }
  '
}

while IFS=$'\t' read -r device image; do
  [[ -z "$device" ]] && continue
  echo "Detaching stale generated DMG mount: $image ($device)"
  hdiutil detach "$device" >/dev/null || hdiutil detach -force "$device" >/dev/null
done < <(mounted_gist_images)

# Tauri normally replaces this file, but remove abandoned partial output from
# interrupted conversions so a retry always starts with a clean destination.
if [[ -d "$DMG_DIR" ]]; then
  find "$DMG_DIR" -maxdepth 1 -type f -name 'Gist_*.dmg' -delete
fi
