#!/usr/bin/env bash
set -euo pipefail

if [ "${1-}" = "" ]; then
  echo "Usage: $(basename "$0") <hex|0xhex>" >&2
  exit 1
fi

command -v python3 >/dev/null || { echo "python3 not found" >&2; exit 127; }

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd)"
exec python3 "$SCRIPT_DIR/b58hex.py" hex2b58 "$1"
