#!/usr/bin/env bash
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
exec "${NORTH_AGENT_PYTHON:-python3}" "$HERE/lib/shellcmd.property.py" --guard corpus-scan
