import os
import sys
import tomllib
from pathlib import Path

root = Path(os.environ.get("DEFAULTS_ROOT") or Path(__file__).resolve().parents[2])
registry = tomllib.loads((root / "harness/defaults.toml").read_text())
KINDS = ("measured: ", "tom: ", "standard: ", "prior: ")
counters = (root / "crates/north-core/src/ops/defaults_measure/priors.rs").read_text()
failures = []
for name, entry in registry.items():
    source, fallback = str(entry.get("source", "")), str(entry.get("fallback", ""))
    if "value" not in entry:
        failures.append(f"{name}: no value")
    if not source.startswith(KINDS) or not source.split(": ", 1)[1].strip():
        failures.append(f"{name}: source {source!r} is not one of {', '.join(k.strip() for k in KINDS)} with a reason")
    if source.startswith("prior: "):
        settles = entry.get("settles_when")
        if not isinstance(settles, dict) or not isinstance(settles.get("need"), int) or settles["need"] < 1:
            failures.append(f"{name}: a prior needs settles_when with a positive integer need")
        elif f'"{settles.get("count")}" =>' not in counters or not str(settles.get("what", "")).strip():
            failures.append(f"{name}: settles_when count {settles.get('count')!r} is not a north defaults-measure counter with a what")
    if not fallback.strip():
        failures.append(f"{name}: empty fallback")
    if not entry.get("consumers"):
        failures.append(f"{name}: no consumer")
    for consumer in entry.get("consumers", []):
        path = root / consumer
        if not path.exists():
            failures.append(f"{name}: consumer {consumer} missing")
        elif f'DEFAULTS["{name}"]' not in path.read_text() and not (path.suffix == ".rs" and any(f'"{key}"' in path.read_text() for key in (name, *(prefix for prefix in ("codex_route_", "codex_tier_") if name.startswith(prefix)))) and any('/harness/defaults.toml")' in module.read_text() for module in {path, *path.parent.glob("*.rs")})):
            failures.append(f"{name}: {consumer} does not read DEFAULTS[\"{name}\"]")
for failure in failures:
    print(f"FAIL {failure}")
if failures:
    sys.exit(1)
consumers = {c for entry in registry.values() for c in entry["consumers"]}
print(f"PASS {len(registry)} defaults each carry a value, a measured/tom/standard source or a prior with settles_when, and a fallback, "
      f"and all {len(consumers)} consuming modules read them from harness/defaults.toml")
