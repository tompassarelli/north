#!/usr/bin/env python3
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import tomllib

SCRIPT = Path(__file__).resolve().parent / "policy-replay.py"
POINT = """date = "2026-10-12T07:19:38Z"
model = "m"
variant = "base"
policy_sha256 = "old"
startup_tokens = 5213
conformance = 0.5
n = 4
evidence = "e"
holdout_from = "2026-10-13"
weak_scenarios = ["b"]
# audit note kept by promote
"""


def run(*args, env):
    return subprocess.run([sys.executable, str(SCRIPT), *map(str, args)], env=env, capture_output=True, text=True)


with tempfile.TemporaryDirectory() as tmp:
    tmp = Path(tmp)
    op, fake = tmp / "agents", tmp / "bin"
    (op / "replay").mkdir(parents=True)
    fake.mkdir()
    (op / "AGENTS.md").write_text("# Policy\n\n## Rules\n\n- Keep it short.\n")
    (op / "catalog-config.json").write_text('{"activation": {}, "registrations": {}, "baselines": []}\n')
    (op / "orchestration.toml").write_text("[projects]\n")
    (op / "operating-point.toml").write_text(POINT)
    (op / "replay/variants.toml").write_text('[[variant]]\nname = "base"\n')
    scenarios = []
    for sid in ("a", "b"):
        path = op / "replay" / f"{sid}.json"
        path.write_text(json.dumps({"id": sid, "prompt": "p", "detector": {"kind": "regex", "pattern": "ok"}}))
        scenarios.append(path)
    (fake / "claude").write_text("#!/bin/sh\n[ \"$1\" = --version ] && { echo 9.9; exit 0; }\n"
                                 "echo '{\"type\":\"result\",\"subtype\":\"success\",\"result\":\"ok\",\"model\":\"m\"}'\n")
    (fake / "claude").chmod(0o755)
    env = dict(os.environ, PATH=f"{fake}:{os.environ['PATH']}", NORTH_FIRN=str(tmp), HOME=str(tmp))
    candidate = tmp / "candidate.toml"

    swept = run(*scenarios, "--sweep", op / "replay/variants.toml", "--candidate", candidate, "--out", tmp / "sweep.jsonl", env=env)
    assert swept.returncode == 0, swept.stderr[-2000:]
    assert (op / "operating-point.toml").read_text() == POINT, "a sweep rewrote the operating point"
    cand = tomllib.loads(candidate.read_text())
    assert cand["scenarios"] == {"a": [1, 1], "b": [1, 1]} and cand["conformance"] == 1.0, cand
    print("ok: a sweep writes only the candidate and leaves operating-point.toml unchanged")

    extra = op / "replay" / "c.json"
    extra.write_text(json.dumps({"id": "c", "prompt": "p", "detector": {"kind": "regex", "pattern": "ok"}}))
    partial = run(*scenarios, extra, "--promote", "--candidate", candidate, env=env)
    assert partial.returncode == 1 and "does not cover ['c']" in partial.stderr, partial
    assert (op / "operating-point.toml").read_text() == POINT
    print("ok: promote refuses a candidate that misses a scenario")

    drop = candidate.read_text().replace("conformance = 1.0", "conformance = 0.0").replace("n = 2", "n = 40")
    (tmp / "drop.toml").write_text(drop)
    dropped = run(*scenarios, "--promote", "--candidate", tmp / "drop.toml", env=env)
    assert dropped.returncode == 1 and "significantly below" in dropped.stderr, dropped
    assert (op / "operating-point.toml").read_text() == POINT

    promoted = run(*scenarios, "--promote", "--candidate", candidate, env=env)
    assert promoted.returncode == 0, promoted.stderr
    text = (op / "operating-point.toml").read_text()
    point = tomllib.loads(text)
    assert point["holdout_from"] == "2026-10-13" and point["weak_scenarios"] == ["b"], point
    assert "# audit note kept by promote" in text, text
    assert point["conformance"] == 1.0 and point["policy_sha256"] == cand["policy_sha256"] and "scenarios" not in point, point
    print("ok: promote refuses a significant conformance drop, then updates the point keeping holdout_from, weak_scenarios and comments")
