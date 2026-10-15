"""worker-ledger's recurring step: threads calibrate --write on the ledger host when the newest calibration row is
older than calibrate_after_min, defaults-measure --priors recorded as kind prior rows on the host holding the
Codex rollouts, once a week from Monday 00:00Z, and codex-route-propose at most once an hour, its failure only reported."""
from datetime import datetime, timedelta, timezone
import os
from pathlib import Path
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
import calibration
from defaults import registry
import provider_capacity

ROOT = Path(__file__).resolve().parents[2]
THREADS = ROOT / "bin/threads"
MEASURE = ROOT / "harness/scripts/defaults-measure.py"
ROUTE_PROPOSE = ROOT / "harness/scripts/codex-route-propose.py"


def learn_state():
    return Path(os.environ.get("XDG_STATE_HOME", Path.home() / ".local/state")) / "agents/learn"


def propose_routes(now):
    mark = learn_state() / "codex-route-propose.last"
    try:
        if now - datetime.fromisoformat(mark.read_text().strip()) < timedelta(hours=1):
            return
    except (OSError, ValueError):
        pass
    mark.parent.mkdir(parents=True, exist_ok=True)
    mark.write_text(now.isoformat())
    try:
        done = subprocess.run([sys.executable, str(ROUTE_PROPOSE), "--state", str(learn_state())],
                              capture_output=True, text=True, timeout=300)
        if done.returncode:
            print(f"recalibrate: codex-route-propose rc={done.returncode} {done.stderr.strip()[-300:]}", file=sys.stderr)
    except (OSError, subprocess.SubprocessError) as error:
        print(f"recalibrate: codex-route-propose {error}", file=sys.stderr)


def prior_specs(out):
    needs = {name: e["settles_when"]["need"] for name, e in registry().items() if "settles_when" in e}
    specs = []
    for line in out.splitlines():
        state, name, n, *rest = line.split() + [""]
        if state not in ("SETTLED", "WAIT") or name not in needs or not n.startswith("n="):
            continue
        value = next((w[6:] for w in rest if w.startswith("value=")), "")
        specs.append(f"{name}:{n[2:].split('/')[0]}:{needs[name]}" + (f":{value}" if value else ""))
    return specs


def main():
    rows, now, code = calibration.rows(), datetime.now(timezone.utc), 0
    if calibration.owns_ledger() and calibration.calibration_due(rows, now):
        done = subprocess.run([sys.executable, str(THREADS), "calibrate", "--write"], stdout=subprocess.DEVNULL)
        print(f"recalibrate: threads calibrate --write rc={done.returncode}", file=sys.stderr)
        code |= done.returncode
    if provider_capacity.root().is_dir() and calibration.priors_due(rows, now):
        done = subprocess.run([sys.executable, str(MEASURE), "--priors", str(calibration.ledger_path())],
                              capture_output=True, text=True, timeout=600)
        specs = prior_specs(done.stdout) if done.returncode == 0 else []
        if specs:
            done = subprocess.run([sys.executable, str(THREADS), "record-priors", *specs])
        print(f"recalibrate: priors {' '.join(specs) or done.stderr.strip()[-300:]} rc={done.returncode}", file=sys.stderr)
        code |= done.returncode or not specs
    propose_routes(now)
    return 1 if code else 0


if __name__ == "__main__":
    sys.exit(main())
