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

    import importlib.util
    spec = importlib.util.spec_from_file_location("policy_replay", SCRIPT)
    replay = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(replay)
    recorded = replay.events((SCRIPT.parent / "policy-replay.advisor.jsonl").read_text())
    adv = replay.advisor_usage(recorded)
    assert adv["advisor_calls"] == 1 and adv["advisor_model_resolved"] == "claude-opus-5-5", adv
    assert adv["advisor_output_tokens"] == 201 and adv["advisor_input_tokens"] == 9902, adv
    assert replay.advisor_usage([e for e in recorded if e.get("type") != "assistant"])["advisor_calls"] == 0
    merged = [dict(e, modelUsage={}) if e.get("type") == "result" else e for e in recorded]
    assert replay.advisor_usage(merged)["advisor_model_resolved"] == "claude-opus-5-5"
    print("ok: advisor_calls and advisor tokens parse from a recorded haiku + opus-advisor stream")

    work = tmp / "work"
    work.mkdir()
    check = {"kind": "command", "command": "test -f built"}
    assert not replay.detect(check, [], work)
    assert not replay.detect({"kind": "command", "command": "exit 2"}, [], work)
    (work / "built").write_text("")
    assert replay.detect(check, [], work)
    assert not replay.detect({"kind": "command", "command": "sleep 5", "timeout_s": 0.5}, [], work)
    print("ok: the command detector passes on exit 0, fails otherwise and on its timeout")

    coding = op / "replay" / "coding"
    coding.mkdir()
    task = coding / "t.json"
    task.write_text(json.dumps({"id": "t", "prompt": "p", "policy": "../../AGENTS.md",
                                "detector": {"kind": "command", "command": "grep -q done out.txt"}}))
    (fake / "claude").write_text("#!/bin/sh\n[ \"$1\" = --version ] && { echo 9.9; exit 0; }\n"
                                 f"echo \"$@\" > {tmp}/argv\necho done > out.txt\n"
                                 "echo '{\"type\":\"result\",\"subtype\":\"success\",\"result\":\"ok\",\"model\":\"m\"}'\n")
    arm = run(task, "--arm", "full", "--model", "haiku", "--effort", "low", "--advisor", "opus", "--out", tmp / "advisor-x.jsonl", env=env)
    assert arm.returncode == 0, arm.stderr[-2000:]
    row = json.loads((tmp / "advisor-x.jsonl").read_text())
    argv = (tmp / "argv").read_text()
    assert "--model haiku" in argv and "--effort low" in argv and "--advisor opus" in argv, argv
    assert row["pass"] is True and (row["model"], row["effort"], row["advisor"], row["advisor_calls"]) == ("haiku", "low", "opus", 0), row
    assert row["usd"] == 0 and row["usd_basis"].startswith("total_cost_usd"), row
    fable = run(task, "--arm", "full", "--advisor", "fable", "--out", tmp / "advisor-y.jsonl", env=env)
    assert fable.returncode != 0 and "Fable" in fable.stderr, fable
    print("ok: a replay/coding scenario with its own policy runs --model/--effort/--advisor and scores through its command")
