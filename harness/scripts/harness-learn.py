import argparse
import fcntl
import json
import math
import os
from pathlib import Path
import re
import shlex
import subprocess
import sys
import tempfile
import tomllib
import uuid
from datetime import datetime, timedelta, timezone

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "harness/lib"))
from defaults import DEFAULTS


def run(argv, cwd=None):
    result = subprocess.run(list(map(str, argv)), cwd=cwd, text=True, capture_output=True)
    if result.returncode:
        raise RuntimeError(f"{argv[0]} exited {result.returncode}: {result.stderr.strip() or result.stdout.strip()}")
    return result.stdout.strip()


def stamp(value):
    parsed = datetime.fromisoformat(value.replace("Z", "+00:00"))
    return parsed.replace(tzinfo=parsed.tzinfo or timezone.utc).astimezone(timezone.utc)


def rows(text):
    return [json.loads(line) for line in text.splitlines() if line.strip()]


def collect(args, now):
    proposals, sources = [], []
    fresh = {"produced_at": now.isoformat()}
    reads = [(label, command and (lambda command=command, suffix=suffix: run(shlex.split(command) + suffix, args.repo)), fresh)
             for label, command, suffix in (("defaults", args.defaults_command, []),
                                            ("pace", args.pace_command, ["--propose", "--json"]))]
    reads += [(str(path), path.read_text, {}) for path in dict.fromkeys(sorted((args.state / "proposals").glob("*.jsonl")) + args.proposals)]
    for label, read, extra in reads:
        try:
            if not read:
                raise FileNotFoundError
            proposals.extend({**extra, **row, "dimension": row.get("dimension") or row.get("report") or Path(label).stem}
                             for row in rows(read()))
            sources.append({"source": label, "status": "read"})
        except FileNotFoundError:
            sources.append({"source": label, "status": "absent"})
        except (RuntimeError, ValueError, TypeError) as error:
            sources.append({"source": label, "status": "error", "reason": str(error)})
    return proposals, sources


def judgment(row, minimum, now):
    if row.get("eligible") is not True:
        return "ineligible", row.get("reason") or "owner did not authorize adoption"
    if (row.get("report") == "W5" or row.get("dimension") == "W5") and row.get("default") != "eta_min_samples":
        return "ineligible", "W5 only automates eta_min_samples"
    if not all(key in row for key in ("default", "dimension", "current", "proposed")):
        return "waited", "insufficient evidence: proposal fields missing"
    try:
        held_out = stamp(row["since"]) >= stamp(row["train_until"])
    except (KeyError, ValueError, TypeError, AttributeError):
        held_out = False
    if not held_out:
        return "waited", "in-sample evidence: evaluation window must start at or after train_until"
    try:
        recent = now - timedelta(days=7) <= stamp(row["produced_at"]) <= now
    except (KeyError, ValueError, TypeError, AttributeError):
        recent = False
    if not recent:
        return "waited", "stale evidence: not produced within the last 7 days"
    n = row.get("n")
    if type(n) is not int or n < minimum:
        return "waited", "insufficient evidence"
    numbers = [row.get(key) for key in ("score_current", "score_proposed", "ci_low", "ci_high")]
    if not all(type(value) in (int, float) and math.isfinite(value) for value in numbers):
        return "waited", "insufficient evidence: judged scores and gain CI required"
    if row["ci_low"] > row["ci_high"]:
        return "waited", "invalid gain CI"
    if row["ci_low"] > 0:
        return "won", "gain CI excludes zero"
    if row["ci_high"] <= 0:
        return "rejected", "gain CI excludes a gain"
    return "waited", "inconclusive gain CI"


def replace_default(text, name, current, proposed, source):
    registry = tomllib.loads(text)
    entry = registry.get(name)
    if not entry or entry["value"] != current:
        raise ValueError("default missing or current value changed")
    numeric = type(current) in (int, float) and type(proposed) in (int, float)
    if not numeric and type(current) is not type(proposed):
        raise ValueError("proposed value changes the default's type")
    if type(proposed) is float and not math.isfinite(proposed):
        raise ValueError("proposed value is not finite")
    value = json.dumps(proposed, allow_nan=False)
    section = re.compile(r"(?ms)^\[" + re.escape(name) + r"\]\s*\n.*?(?=^\[|\Z)")
    def edit(match):
        block = re.sub(r"(?m)^value = .*?$", lambda _: "value = " + value, match[0], count=1)
        return re.sub(r"(?m)^source = .*?$", lambda _: "source = " + json.dumps(source), block, count=1)
    updated, count = section.subn(edit, text)
    if count != 1 or tomllib.loads(updated)[name]["value"] != proposed:
        raise ValueError("default does not have an editable value/source section")
    return updated


def land(args, row, now, revert=False):
    run(["git", "fetch", "origin", "main"], args.repo)
    branch = "harness-learn-" + uuid.uuid4().hex
    with tempfile.TemporaryDirectory(prefix="harness-learn-") as directory:
        lane = Path(directory) / "lane"
        run(["git", "worktree", "add", "-b", branch, lane, "origin/main"], args.repo)
        try:
            path = lane / "harness/defaults.toml"
            source = (f"measured: {now.date()} harness-learn: n={row['n']}, "
                      f"score {row['score_current']}->{row['score_proposed']} "
                      f"[{row['ci_low']}, {row['ci_high']}]" + (" reverted" if revert else ""))
            old, new = (row["proposed"], row["current"]) if revert else (row["current"], row["proposed"])
            path.write_text(replace_default(path.read_text(), row["default"], old, new, source))
            run([sys.executable, "harness/lib/defaults.test.py"], lane)
            run(["git", "add", "--", "harness/defaults.toml"], lane)
            run(["git", "commit", "-m", f"harness-learn: {'revert' if revert else 'adopt'} {row['default']}"], lane)
            run([args.safe_push, "--to", "main"], lane)
            run(["git", "fetch", "origin", "main"], args.repo)
            sha = run(["git", "rev-parse", f"{branch}^{{commit}}"], args.repo)
            run(["git", "merge-base", "--is-ancestor", sha, "origin/main"], args.repo)
            return sha
        finally:
            if lane.exists():
                run(["git", "worktree", "remove", "--force", lane], args.repo)
            run(["git", "branch", "-D", branch], args.repo)


def history(path):
    active, reverted, failed = {}, {}, {}
    if path.exists():
        for cycle in rows(path.read_text()):
            for row in cycle["proposals"]:
                key = row.get("default")
                if row["verdict"] == "failed":
                    failed[(row.get("action"), key, json.dumps(row["proposed"]))] = stamp(cycle["time"]).date()
                elif row["verdict"] == "landed":
                    active[key] = row
                elif row["verdict"] in ("reverted", "kept"):
                    active.pop(key, None)
                    if row["verdict"] == "reverted":
                        reverted[(key, json.dumps(row["proposed"], sort_keys=True))] = max(row["n"], row.get("trial_n", 0))
    return active, reverted, failed


def oriented(row, trial):
    if (row.get("default"), row.get("dimension")) != (trial["default"], trial["dimension"]):
        return None
    if (row.get("current"), row.get("proposed")) == (trial["current"], trial["proposed"]):
        return row
    if (row.get("current"), row.get("proposed")) != (trial["proposed"], trial["current"]):
        return None
    flipped = dict(row, current=row["proposed"], proposed=row["current"],
                   score_current=row.get("score_proposed"), score_proposed=row.get("score_current"))
    low, high = row.get("ci_low"), row.get("ci_high")
    if type(low) in (int, float) and type(high) in (int, float):
        flipped.update(ci_low=-high, ci_high=-low)
    return flipped


def attempt(args, row, now, revert=False):
    try:
        return land(args, row, now, revert), None
    except ValueError as error:
        return None, ("waited", str(error))
    except Exception as error:
        return None, ("failed", f"{type(error).__name__}: {error}")


def cycle(args):
    now = stamp(args.now) if args.now else datetime.now(timezone.utc)
    minimum = DEFAULTS["learn_min_n"]
    verdicts, sources = [], []
    try:
        active, reverted, failed = history(args.state / "cycles.jsonl")
        tried = lambda row: failed.get((row["action"], row.get("default"), json.dumps(row.get("proposed")))) == now.date()
        proposals, sources = collect(args, now)
        changed = set()
        for name, trial in active.copy().items():
            result = dict(trial, action="rejudge", verdict="waited", reason="awaiting fresh post-landing evidence")
            if trial["dimension"] in changed:
                result["reason"] = "dimension already changed this cycle"
            elif stamp(trial["landed_at"]).date() < now.date():
                for candidate in proposals:
                    row = oriented(candidate, trial)
                    if row is None:
                        continue
                    try:
                        if not stamp(trial["landed_at"]) <= stamp(row.get("since", "")) <= now:
                            continue
                    except (ValueError, TypeError):
                        continue
                    verdict, reason = judgment(row, minimum, now)
                    result = dict(row, action="rejudge", trial_sha=trial["sha"], trial_n=trial["n"],
                                  verdict="waited", reason=reason)
                    if verdict == "rejected":
                        sha, failure = (None, ("waited", "landing failed today")) if tried(result) else attempt(args, row, now, revert=True)
                        if failure:
                            result["verdict"], result["reason"] = failure
                        else:
                            result.update(sha=sha, verdict="reverted")
                            reverted[(name, json.dumps(row["proposed"], sort_keys=True))] = max(row["n"], trial["n"])
                            changed.add(row["dimension"])
                            active.pop(name)
                        break
                    if verdict == "won":
                        result["verdict"] = "kept"
                        active.pop(name)
                        break
            verdicts.append(result)
        for row in proposals:
            result = dict(row, action="propose")
            verdict, reason = judgment(row, minimum, now)
            name, dimension = row.get("default"), row.get("dimension")
            if verdict != "ineligible" and name in active:
                verdict, reason = "waited", "knob has an active trial"
            if verdict == "won":
                previous = reverted.get((name, json.dumps(row.get("proposed"), sort_keys=True)))
                if dimension in changed:
                    verdict, reason = "waited", "dimension already changed this cycle"
                elif previous is not None and row["n"] < previous + minimum:
                    verdict, reason = "waited", "reverted value needs more evidence"
                elif row.get("current") == row.get("proposed"):
                    verdict, reason = "waited", "unchanged value"
                elif tried(result):
                    verdict, reason = "waited", "landing failed today"
                else:
                    sha, failure = attempt(args, row, now)
                    if failure:
                        verdict, reason = failure
                    else:
                        result.update(sha=sha, landed_at=datetime.now(timezone.utc).isoformat())
                        verdict = "landed"
                        changed.add(dimension)
                        active[name] = result
            result.update(verdict=verdict, reason=reason)
            verdicts.append(result)
    finally:
        record = {"time": now.isoformat(), "learn_after_landings": DEFAULTS["learn_after_landings"],
                  "sources": sources, "proposals": verdicts}
        with (args.state / "cycles.jsonl").open("a") as stream:
            stream.write(json.dumps(record, allow_nan=False, default=str) + "\n")
            stream.flush()
            os.fsync(stream.fileno())
        body = " | ".join(
            f"{row.get('dimension')}: {row.get('default')} {row.get('current')}->{row.get('proposed')} "
            + (f"{row['verdict']} {row['sha']}" if row["verdict"] in ("landed", "reverted") else
               f"{row['verdict']} (n={row.get('n')}, score={row.get('score_current')}->{row.get('score_proposed')}, "
               f"ci=[{row.get('ci_low')}, {row.get('ci_high')}]; {row['reason']})")
            for row in verdicts) or "nothing judged"
        run([args.north, "fleet", "send", "proxy", f"LEARN {body}"])
        print(body)


def main():
    parser = argparse.ArgumentParser(description="Judge supplied gain CIs and land one default per dimension.")
    parser.add_argument("--repo", type=Path, default=ROOT)
    parser.add_argument("--state", type=Path, default=Path(os.environ.get("XDG_STATE_HOME", Path.home() / ".local/state")) / "agents/learn")
    parser.add_argument("--proposals", type=Path, action="append", default=[])
    parser.add_argument("--defaults-command", help="complete defaults-measure proposal command including holdout arguments")
    parser.add_argument("--pace-command", help="pace_replay command; --propose --json are appended")
    parser.add_argument("--safe-push", default=str(ROOT / "bin/safe-push"))
    parser.add_argument("--north", default="north")
    parser.add_argument("--now")
    args = parser.parse_args()
    args.repo = args.repo.resolve()
    ledger = Path(os.environ.get("THREADS_DB", Path.home() / ".local/state/threads/threads.db"))
    now = stamp(args.now) if args.now else datetime.now(timezone.utc)
    if args.defaults_command is None and ledger.exists():
        window = (now - timedelta(days=7)).isoformat()
        args.defaults_command = shlex.join([args.north, "defaults-measure", str(ledger), "--propose",
                                           "--train-until", window, "--since", window, "--json"])
    pace_source = args.repo / "harness/lib/pace_replay.py"
    if args.pace_command is None and pace_source.exists():
        args.pace_command = shlex.join([sys.executable, str(pace_source)])
    args.state.mkdir(parents=True, exist_ok=True)
    with (args.state / "cycle.lock").open("a") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            return
        cycle(args)


if __name__ == "__main__":
    main()
