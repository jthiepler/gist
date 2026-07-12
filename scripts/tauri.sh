#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [[ "${1:-}" == "build" ]]; then
  "$SCRIPT_DIR/prepare-dmg.sh"
fi

exec tauri "$@"
