#!/usr/bin/env bash
set -euo pipefail

gate="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/dep-gate"
wisp="${1:-$HOME/code/wisp/main}"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/dep-gate-wisp.XXXXXX")"
trap 'rm -rf "${scratch:?}"' EXIT
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
printf 'ok: Wisp recorded dependency scenario exits 1, 0, 1\n'
python3 - "$gate" <<'PY'
import runpy
import sys

gate = runpy.run_path(sys.argv[1])
now = 2_000_000_000
before = {
    "Cargo.lock": '[[package]]\nname = "recorded-crate"\nversion = "1.0.0"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\n',
    "flake.lock": '{"nodes":{"input":{"locked":{"rev":"old","lastModified":1}},"root":{"inputs":{"input":"input"}}}}',
}
after = {
    "Cargo.lock": '[[package]]\nname = "recorded-crate"\nversion = "1.1.0"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\n[[package]]\nname = "local"\nversion = "1.0.0"\n',
    "bun.lock": '''{
      // Recorded registry alias, old boundary release and non-registry URL.
      "packages": {
        "alias": ["@scope/recorded@1.2.3", "", {}, "sha512-recorded",],
        "boundary": ["boundary@1.0.0", "", {}, "sha512-recorded",],
        "remote": ["remote@https://example.com/a,}.tgz", {},], /* kept out */
      },
    }''',
    "flake.lock": '{"nodes":{"input":{"locked":{"rev":"new","lastModified":2000000000}},"root":{"inputs":{"input":"input"}}}}',
}
candidates = gate["lock_candidates"](before, after)
times = {("cargo", "recorded-crate", "1.1.0"): now - 60,
         ("npm", "@scope/recorded", "1.2.3"): now - 60,
         ("npm", "boundary", "1.0.0"): now - 604800}
failures = gate["age_violations"](candidates, times, now, set())
assert len(failures) == 3, failures
for identity in ("recorded-crate@1.1.0", "@scope/recorded@1.2.3", "input@new"):
    assert sum(identity in line for line in failures) == 1, failures
urgent = gate["urgent_dependencies"](["Recorded advisory fix\n\nDependency-Urgent: recorded-crate@1.1.0 RUSTSEC-2026-0001\nDependency-Urgent: @scope/recorded@1.2.3 GHSA-recorded\nDependency-Urgent: input@new GHSA-recorded"])
assert gate["age_violations"](candidates, times, now, urgent) == []
assert gate["age_violations"](candidates, {("npm", "boundary", "1.0.0"): now - 604800}, now, urgent) == []
after["flake.lock"] = '{"nodes":{"input":{"locked":{"rev":"new"}}}}'
failures = gate["age_violations"](gate["lock_candidates"](before, after), times, now, urgent)
assert len(failures) == 1 and "missing lastModified" in failures[0], failures
after["flake.lock"] = '{"nodes":{"firn":{"locked":{"type":"github","owner":"tompassarelli","repo":"firn","rev":"new","lastModified":2000000000}},"local":{"locked":{"type":"path","path":"/home/recorded/code/clause","lastModified":2000000000}},"input":{"locked":{"type":"github","owner":"someone","repo":"input","rev":"new","lastModified":2000000000}}}}'
failures = gate["age_violations"]([c for c in gate["lock_candidates"](before, after) if c.ecosystem == "flake"], times, now, set())
assert len(failures) == 1 and "input@new" in failures[0], failures
print("ok: recorded lock ages reject Cargo, scoped Bun and third-party flake, allow first-party flake, seven-day boundary and urgent trailers")
PY
