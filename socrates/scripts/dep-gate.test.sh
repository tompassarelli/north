#!/usr/bin/env bash
set -euo pipefail

gate="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/dep-gate"
wisp="${1:-$HOME/code/wisp/main}"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/dep-gate-wisp.XXXXXX")"
git clone --quiet --shared --no-checkout "$wisp" "$scratch"
cd "$scratch"
base="$(git rev-parse HEAD)"
git read-tree HEAD
git show HEAD:package.json >package.json
python3 - <<'PY'
import json
from pathlib import Path

path = Path("package.json")
data = json.loads(path.read_text())
deps = data.setdefault("dependencies", {})
assert "left-pad" not in deps
deps["left-pad"] = "1.3.0"
path.write_text(json.dumps(data, indent=2) + "\n")
PY
git add package.json
git commit --quiet -m 'Record dependency addition without a reason'
set +e
"$gate" "$base" HEAD >missing.out 2>&1
missing=$?
set -e
[[ "$missing" == 1 ]]
rg -q 'left-pad: added dependency needs a reason' missing.out
git commit --quiet --amend -m $'Record dependency addition with a reason\n\nDependency: left-pad — exercise the dependency gate'
"$gate" "$base" HEAD >reason.out 2>&1
[[ ! -s reason.out ]]
python3 - <<'PY'
import json
from pathlib import Path

path = Path("package.json")
data = json.loads(path.read_text())
assert "x" not in data["dependencies"]
data["dependencies"]["x"] = "github:someone/x"
path.write_text(json.dumps(data, indent=2) + "\n")
PY
git add package.json
git commit --quiet -m $'Record forbidden source with a reason\n\nDependency: x - exercise the source restriction'
set +e
"$gate" "$base" HEAD >source.out 2>&1
source=$?
set -e
[[ "$source" == 1 ]]
rg -q 'x: forbidden new dependency source' source.out
printf 'ok: Wisp recorded dependency scenario exits 1, 0, 1; scratch %s\n' "$scratch"
