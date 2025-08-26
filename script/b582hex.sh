#!/usr/bin/env bash
set -euo pipefail

if [ "${1-}" = "" ]; then
  echo "Usage: $(basename "$0") <base58>" >&2
  exit 1
fi

command -v python3 >/dev/null || { echo "python3 not found" >&2; exit 127; }

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd)"
hex="$(python3 "$SCRIPT_DIR/b58hex.py" b582hex "$1")"
printf '0x%s\n' "$hex"
