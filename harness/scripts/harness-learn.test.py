import fcntl
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import tomllib
import unittest
from datetime import datetime, timedelta, timezone

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "harness/scripts/harness-learn.py"


class LearningCycle(unittest.TestCase):
    def test_only_judged_gains_land_and_fresh_losses_revert_without_reusing_evidence(self):
        with tempfile.TemporaryDirectory(prefix="learn-test-") as directory:
            root = Path(directory)
            repo, remote, state = root / "repo", root / "origin.git", root / "state"
            env = dict(os.environ, XDG_STATE_HOME=str(root / "xdg"),
                       THREADS_DB=str(root / "threads.db"), LEDGER_HANDOFFS=str(root / "handoffs"),
                       CODEX_HOME=str(root / "codex"), PATH=f"{ROOT / os.environ.get('CARGO_TARGET_DIR', 'target') / 'debug'}:{os.environ['PATH']}")
            for key in ("GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE"):
                env.pop(key, None)

            def command(*argv, cwd=repo):
                result = subprocess.run(list(map(str, argv)), cwd=cwd, env=env, text=True, capture_output=True)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                return result.stdout.strip()

            repo.mkdir()
            command("git", "init", "--bare", "--initial-branch=main", remote)
            command("git", "init", "--initial-branch=main")
            command("git", "config", "user.name", "Learning fixture")
            command("git", "config", "user.email", "learn@example.invalid")
            for part in ("harness/lib", "harness/scripts", "bin"):
                (repo / part).mkdir(parents=True)
            registry = tomllib.loads((ROOT / "harness/defaults.toml").read_text())
            for path in {"harness/defaults.toml", "harness/lib/defaults.test.py", "harness/scripts/defaults-measure.py"} | {
                    consumer for entry in registry.values() for consumer in entry["consumers"]}:
                (repo / path).parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(ROOT / path, repo / path)
            command("git", "add", ".")
            command("git", "commit", "-m", "fixture")
            command("git", "remote", "add", "origin", remote)
            command("git", "push", "-u", "origin", "main")
            command("git", "symbolic-ref", "refs/remotes/origin/HEAD", "refs/remotes/origin/main")
            base = command("git", "rev-parse", "HEAD")
            events = root / "events.jsonl"
            north = root / "north"
            north.write_text("#!/usr/bin/env python3\nimport json, sys\nwith open(" + repr(str(events))
                              + ", 'a') as stream: stream.write(json.dumps(sys.argv[1:]) + '\\n')\n")
            north.chmod(0o755)
            state.mkdir()
            (state / "proposals").mkdir()
            fixture = state / "proposals" / "ETA.jsonl"
            current = registry["eta_min_samples"]["value"]
            today = datetime.now(timezone.utc)
            tomorrow = (today + timedelta(days=1)).isoformat()

            def proposal(**changes):
                return dict({"dimension": "ETA", "default": "eta_min_samples", "current": current,
                             "proposed": current + 1, "n": 30, "score_current": 0.8, "score_proposed": 0.9,
                             "ci_low": 0.02, "ci_high": 0.18, "eligible": True, "reason": "paired held-out fixture",
                             "train_until": (today - timedelta(days=8)).isoformat(),
                             "since": (today - timedelta(days=7)).isoformat(), "produced_at": today.isoformat()},
                            **changes)

            def execute(proposals, now=None, extra=()):
                fixture.write_text("".join(json.dumps(row) + "\n" for row in proposals))
                before = len(events.read_text().splitlines()) if events.exists() else 0
                argv = [sys.executable, SCRIPT, "--repo", repo, "--state", state, "--north", north,
                        "--safe-push", ROOT / "bin/safe-push", *extra]
                if now:
                    argv += ["--now", now]
                output = command(*argv)
                published = [json.loads(line) for line in events.read_text().splitlines()]
                self.assertEqual(len(published), before + 1)
                self.assertEqual(published[-1], ["fleet", "send", "proxy", "LEARN " + output])
                record = json.loads((state / "cycles.jsonl").read_text().splitlines()[-1])
                return record["proposals"]

            first = execute([proposal(), proposal(default="eta_coverage", current=registry["eta_coverage"]["value"], proposed=0.8),
                             proposal(dimension="routing", default="min_runs", current=5, proposed=6, ci_low=-0.2, ci_high=-0.01),
                             proposal(dimension="policy", default="success_threshold", current=80, proposed=70, eligible=False)])
            self.assertEqual([row["verdict"] for row in first], ["landed", "waited", "rejected", "ineligible"])
            landed = command("git", "rev-parse", "origin/main")
            self.assertEqual(first[0]["sha"], landed)
            self.assertEqual(command("git", "rev-list", "--count", f"{base}..{landed}"), "1")
            self.assertEqual(command("git", "diff", "--name-only", base, landed), "harness/defaults.toml")
            before, after = registry, tomllib.loads(command("git", "show", f"{landed}:harness/defaults.toml"))
            expected = json.loads(json.dumps(before))
            expected["eta_min_samples"].update(value=current + 1, source=after["eta_min_samples"]["source"])
            self.assertEqual(after, expected)
            self.assertIn("harness-learn:", after["eta_min_samples"]["source"])
            self.assertEqual(execute([proposal(proposed=current + 2)])[1]["reason"], "knob has an active trial")
            stale = proposal(ci_low=-0.3, ci_high=-0.1, since="2000-01-01T00:00:00Z")
            self.assertEqual(execute([stale], tomorrow)[0]["verdict"], "waited")
            fresh = proposal(since=first[0]["landed_at"], ci_low=-0.05, ci_high=0.15)
            self.assertEqual(execute([fresh], tomorrow)[0]["verdict"], "waited")
            self.assertEqual(command("git", "rev-parse", "origin/main"), landed)
            loss = dict(fresh, ci_low=-0.3, ci_high=-0.1)
            reverted = execute([loss], tomorrow)
            self.assertEqual(reverted[0]["verdict"], "reverted")
            revert_sha = command("git", "rev-parse", "origin/main")
            self.assertEqual(command("git", "rev-list", "--count", f"{landed}..{revert_sha}"), "1")
            self.assertEqual(command("git", "rev-parse", f"{revert_sha}^"), landed)
            restored = tomllib.loads(command("git", "show", f"{revert_sha}:harness/defaults.toml"))
            self.assertEqual(restored["eta_min_samples"]["value"], current)
            self.assertEqual(execute([proposal(n=49)], tomorrow)[0]["reason"], "reverted value needs more evidence")
            adopted = execute([proposal(n=50)], tomorrow)[0]
            self.assertEqual(adopted["verdict"], "landed")
            kept = execute([proposal(n=20, since=adopted["landed_at"])], tomorrow)
            self.assertEqual(kept[0]["verdict"], "kept")
            missing_ci = proposal(dimension="B1", default="eta_coverage", current=0.75, proposed=0.8)
            missing_ci.pop("ci_low")
            self.assertEqual(execute([missing_ci], tomorrow)[0]["verdict"], "waited")
            self.assertEqual(execute([proposal(n=19)], tomorrow)[0]["verdict"], "waited")
            self.assertEqual(execute([], tomorrow), [])
            self.assertEqual(json.loads(events.read_text().splitlines()[-1])[-1], "LEARN nothing judged")
            source = root / "source.py"
            source.write_text("import json\nprint(json.dumps(" + repr(proposal(eligible=False)) + "))\n")
            supplied = execute([], tomorrow, ["--defaults-command", f"{sys.executable} {source}",
                                               "--pace-command", str(root / "missing")])
            self.assertEqual(supplied[0]["verdict"], "ineligible")
            in_sample = proposal(dimension="B1", default="eta_coverage", train_until=today.isoformat())
            self.assertEqual(execute([in_sample], tomorrow)[0]["reason"],
                             "in-sample evidence: evaluation window must start at or after train_until")
            stale = proposal(dimension="B1", default="eta_coverage", produced_at=(today - timedelta(days=9)).isoformat())
            self.assertEqual(execute([stale], tomorrow)[0]["reason"], "stale evidence: not produced within the last 7 days")
            standing = proposal(current=current + 1, proposed=current + 2, n=56)
            relanded = execute([standing])[0]
            self.assertEqual(relanded["verdict"], "landed")
            reversed_loss = proposal(current=current + 2, proposed=current + 1, n=20, since=relanded["landed_at"],
                                     score_current=0.7, score_proposed=0.9, ci_low=0.05, ci_high=0.3)
            flipped = execute([reversed_loss], tomorrow)[0]
            self.assertEqual((flipped["verdict"], flipped["current"], flipped["proposed"], flipped["ci_low"], flipped["ci_high"]),
                             ("reverted", current + 1, current + 2, -0.3, -0.05))
            self.assertEqual(tomllib.loads(command("git", "show", "origin/main:harness/defaults.toml"))["eta_min_samples"]["value"],
                             current + 1)
            self.assertEqual(execute([standing], tomorrow)[0]["reason"], "reverted value needs more evidence")
            failing = root / "failing-safe-push"
            failing.write_text("#!/usr/bin/env bash\ngit log -1 --format=%s | grep -q eta_coverage && exit 1\nexec "
                               + str(ROOT / "bin/safe-push") + ' "$@"\n')
            failing.chmod(0o755)
            before_failure = command("git", "rev-parse", "origin/main")
            partial = execute([proposal(current=current + 1, proposed=current + 3),
                               proposal(dimension="B1", default="eta_coverage", current=registry["eta_coverage"]["value"], proposed=0.8)],
                              None, ["--safe-push", failing])
            self.assertEqual([row["verdict"] for row in partial], ["landed", "failed"])
            self.assertIn("RuntimeError", partial[1]["reason"])
            body = json.loads(events.read_text().splitlines()[-1])[-1]
            self.assertIn(f"landed {partial[0]['sha']}", body)
            self.assertIn("failed (", body)
            self.assertEqual(command("git", "rev-list", "--count", f"{before_failure}..origin/main"), "1")
            self.assertEqual(execute([proposal(current=current + 1, proposed=current + 3)], tomorrow)[1]["reason"],
                             "knob has an active trial")
            cycles = len((state / "cycles.jsonl").read_text().splitlines())
            with (state / "cycle.lock").open("a") as lock:
                fcntl.flock(lock, fcntl.LOCK_EX)
                try:
                    process = subprocess.run([sys.executable, SCRIPT, "--repo", str(repo), "--state", str(state),
                                              "--north", str(north)], env=env, capture_output=True, text=True, timeout=10)
                finally:
                    fcntl.flock(lock, fcntl.LOCK_UN)
                self.assertEqual((process.returncode, process.stdout, process.stderr), (0, "", ""))
            self.assertEqual(len((state / "cycles.jsonl").read_text().splitlines()), cycles)
            self.assertEqual(command("git", "status", "--porcelain"), "")
            self.assertEqual(len(command("git", "worktree", "list").splitlines()), 1)
            self.assertEqual(command("git", "branch", "--list", "harness-learn-*"), "")
            published = [json.loads(line) for line in events.read_text().splitlines()]
            self.assertEqual(published[-1][:3], ["fleet", "send", "proxy"])
            self.assertTrue(published[-1][3].startswith("LEARN ETA:"))
            print("PASS scratch bare origin: gain-only adoption, dimension/trial limits, fresh CI reversion, evidence growth, one LEARN per cycle, held-out and recent evidence, reversed re-judge rows, failed landings recorded, non-blocking lock, cleanup")


if __name__ == "__main__":
    unittest.main()
