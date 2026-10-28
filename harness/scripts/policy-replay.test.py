#!/usr/bin/env python3
import json
import os
import select
import signal
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
    assert point["weak_scenarios"] == ["b"] and "holdout_from" not in point, point
    assert "# audit note kept by promote" in text, text
    assert point["conformance"] == 1.0 and point["policy_sha256"] == cand["policy_sha256"] and "scenarios" not in point, point
    print("ok: promote refuses a significant conformance drop, then updates the point keeping weak_scenarios and comments, with no holdout_from key")

    def cand(name, conformance, n, scenarios):
        path = tmp / f"{name}.toml"
        path.write_text(f'date = "{name}"\nmodel = "m"\nvariant = "{name}"\npolicy_sha256 = "{name}"\nstartup_tokens = 1\n'
                        f'conformance = {conformance}\nn = {n}\nevidence = "{tmp / "ev.jsonl"}"\n\n[scenarios]\n'
                        + "".join(f'"{s}" = {v}\n' for s, v in scenarios.items()))
        return path

    chain = tmp / "chain.toml"
    chain.write_text(f'date = "d0"\nmodel = "m"\nvariant = "v0"\npolicy_sha256 = "v0"\nstartup_tokens = 1\nconformance = 0.881\nn = 42\nevidence = "{tmp / "ev.jsonl"}"\n')
    (tmp / "ev.jsonl").write_text(json.dumps({"policy_sha256": "v0", "scenarios": {"a": [3, 3], "b": [3, 3]}}) + "\n")
    flip = run(*scenarios, "--promote", "--operating-point", chain, "--candidate", cand("flip", 0.881, 42, {"a": [0, 3], "b": [3, 3]}), env=env)
    assert flip.returncode == 1 and "flips a to 0 passes" in flip.stderr, flip
    step = run(*scenarios, "--promote", "--operating-point", chain, "--candidate", cand("s1", 0.7143, 42, {"a": [3, 3], "b": [2, 3]}), env=env)
    assert step.returncode == 0, step.stderr
    ratchet = run(*scenarios, "--promote", "--operating-point", chain, "--candidate", cand("s2", 0.6905, 42, {"a": [3, 3], "b": [2, 3]}), env=env)
    assert ratchet.returncode == 1 and "below the anchor of d0 0.881" in ratchet.stderr, ratchet
    accepted = run(*scenarios, "--promote", "--operating-point", chain, "--accept-drop", "model release", "--candidate", tmp / "s2.toml", env=env)
    assert accepted.returncode == 0, accepted.stderr
    point = tomllib.loads(chain.read_text())
    assert point["accepted_drop"] == "model release" and (point["anchor_conformance"], point["anchor_n"]) == (0.6905, 42), point
    print("ok: promote refuses a 3/3 to 0/3 flip and a ratchet 0.881 -> 0.714 -> 0.69 against the anchor; --accept-drop records the reason and re-anchors")

    sys.path.insert(0, str(SCRIPT.parent))
    from policy_stats import operating_point
    ids = [f"s{i}" for i in range(14)]
    best = {"variant": "base", "startup_tokens": 2, "conformance": 1.0, "n": 42, "scenarios": {s: [3, 3] for s in ids}}
    lean = {"variant": "lean", "startup_tokens": 1, "conformance": 0.9286, "n": 42, "scenarios": {s: [0 if s == "s0" else 3, 3] for s in ids}}
    assert operating_point([best, lean])["variant"] == "base"
    print("ok: sweep selection skips a cheaper variant that flips one scenario 3/3 to 0/3")

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

    fixtures = SCRIPT.parent / "fixtures"
    probe = replay.events((fixtures / "haiku-opus-advisor.stream.jsonl").read_text())
    debug_log = (fixtures / "haiku-opus-advisor.debug-advisor-lines.log").read_text()
    detached_log = "".join(l for l in debug_log.splitlines(keepends=True) if "Server-side tool enabled" not in l)
    assert replay.advisor_usage(probe)["advisor_calls"] == 1 and replay.advisor_usage(probe)["advisor_errors"] == {}
    assert replay.meter(probe) == {"five_hour_pct": 3, "seven_day_pct": 66}, replay.meter(probe)
    assert replay.advisor_debug(debug_log) == {"advisor_attached": True, "advisor_model_resolved": "claude-opus-5-5"}
    assert replay.advisor_debug(detached_log) == {"advisor_attached": False, "advisor_model_resolved": None}
    failed = [dict(e, message=dict(e["message"], content=[{"type": "advisor_tool_result", "content": {"type": "advisor_tool_result_error", "error_code": code}}]))
              for e in probe[1:2] for code in ("overloaded", "overloaded", "max_uses_exceeded")]
    assert replay.advisor_usage(probe + failed)["advisor_errors"] == {"overloaded": 2, "max_uses_exceeded": 1}
    print("ok: the recorded probe run (north36-probe) gives 1 advisor call, attachment from its debug log, 3% and 66% meter readings and per-code advisor errors")

    work = tmp / "work"
    work.mkdir()
    check = {"kind": "command", "command": "test -f built"}
    assert not replay.detect(check, [], work)
    assert not replay.detect({"kind": "command", "command": "exit 2"}, [], work)
    (work / "built").write_text("")
    assert replay.detect(check, [], work)
    assert not replay.detect({"kind": "command", "command": "sleep 5", "timeout_s": 0.5}, [], work)
    print("ok: the command detector passes on exit 0, fails otherwise and on its timeout")

    group_file = tmp / "group.pid"
    group_fixture = tmp / "group.py"
    group_fixture.write_text("import os,signal,subprocess,sys\nfrom pathlib import Path\n"
                             "signal.signal(signal.SIGTERM,signal.SIG_IGN)\n"
                             "child=subprocess.Popen([sys.executable,'-c',"
                             "\"import os,signal; signal.signal(signal.SIGTERM,signal.SIG_IGN); os.write(1,b'ready'); signal.pause()\"],"
                             "env={},stdout=subprocess.PIPE)\n"
                             "assert child.stdout.read(5)==b'ready'\n"
                             f"Path({str(group_file)!r}).write_text(str(os.getpgrp()))\n"
                             "if ready := os.environ.get('GROUP_READY'):\n"
                             " fd=os.open(ready,os.O_WRONLY); os.write(fd,(str(os.getpgrp())+'\\n').encode()); os.close(fd)\n"
                             "if os.environ.get('GROUP_EXIT'): sys.exit(0)\n"
                             "signal.pause()\n")
    (fake / "claude").write_text("#!/bin/sh\n[ \"$1\" = --version ] && { echo 9.9; exit 0; }\n"
                                 f"exec {sys.executable} {group_fixture}\n")
    timed = op / "replay" / "timed.json"
    timed.write_text(json.dumps({"id": "timed", "prompt": "p", "timeout_s": 1,
                                 "detector": {"kind": "regex", "pattern": "ok"}}))
    stopped = run(timed, "--arm", "full", "--out", tmp / "timed.jsonl", env=env)
    group = int(group_file.read_text())
    try:
        row = json.loads((tmp / "timed.jsonl").read_text())
        survivors = subprocess.run(["pgrep", "-g", str(group)], capture_output=True, text=True)
        assert stopped.returncode == 1 and row["pass"] is None and "timeout" in row["error"], (stopped, row)
        assert survivors.returncode == 1 and not survivors.stdout, survivors.stdout
    finally:
        try:
            os.killpg(group, signal.SIGKILL)
        except ProcessLookupError:
            pass
    assert not replay.run_check(f"{sys.executable} {group_fixture}", work, 1)
    group = int(group_file.read_text())
    try:
        survivors = subprocess.run(["pgrep", "-g", str(group)], capture_output=True, text=True)
        assert survivors.returncode == 1 and not survivors.stdout, survivors.stdout
    finally:
        try:
            os.killpg(group, signal.SIGKILL)
        except ProcessLookupError:
            pass
        while True:
            try:
                os.waitpid(-group, 0)
            except ChildProcessError:
                break
    print("ok: replay and command timeout kill and reap the whole group, including an untagged child ignoring SIGTERM; pgrep -g finds zero survivors")

    import time
    grace_ready = tmp / "grace-ready"
    os.mkfifo(grace_ready)
    grace_fd = os.open(grace_ready, os.O_RDWR | os.O_NONBLOCK)
    try:
        for stop_by in ("grace", "cap"):
            proc = replay.start_group([sys.executable, str(group_fixture)], cwd=work,
                                      env=dict(os.environ, GROUP_READY=str(grace_ready)),
                                      stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            try:
                assert select.select([grace_fd], [], [], 10)[0], "soft-stop group did not become ready"
                assert int(os.read(grace_fd, 4096).strip()) == proc.pid
                now = time.monotonic()
                measurement = {"p75_points": .105, "p75_s": 600}
                assert replay.soft_stop(88, 83, measurement, now=now) == []
                assert proc.poll() is None, "worker whose remaining p75 fits must finish"
                first = replay.soft_stop(88, 88.99, measurement, now=now)
                assert first == ["graceful"]
                assert replay.soft_stop(88, 88.99, measurement, now=now + 299) == []
                assert proc.poll() is None, "299s and cap+0.99 must retain the live group"
                hard = replay.soft_stop(88, 88.99 if stop_by == "grace" else 89.01, measurement,
                                        now=now + (300 if stop_by == "grace" else 299))
                assert hard == ["hard"], hard
                survivors = subprocess.run(["pgrep", "-g", str(proc.pid)], capture_output=True, text=True)
                assert survivors.returncode == 1 and not survivors.stdout, survivors.stdout
            finally:
                replay.stop_group(proc)
    finally:
        os.close(grace_fd)
    print("ok: remaining p75 fits reserve; 299s/cap+0.99 stays alive; 300s or cap+1.01 reaps every group member")

    variants = tmp / "signal-variants"
    variants.mkdir()
    (variants / "variants.toml").write_text('[[variant]]\nname="baseline"\n[[variant]]\nname="e1"\n')
    ready = tmp / "group-ready"
    os.mkfifo(ready)
    ready_fd = os.open(ready, os.O_RDWR | os.O_NONBLOCK)
    tuner = subprocess.Popen([sys.executable, str(SCRIPT.parent / "harness-tuner.py"), "replay", str(scenarios[0]),
                              "--variants", str(variants), "--base", str(op / "AGENTS.md"),
                              "--jobs", "2", "--out", str(tmp / "signal.jsonl")],
                             env=dict(env, GROUP_READY=str(ready)), stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    groups, pending = [], b""
    try:
        while len(groups) < 2:
            assert select.select([ready_fd], [], [], 10)[0], "tuner run groups did not become ready"
            pending += os.read(ready_fd, 4096)
            while b"\n" in pending:
                group, _, pending = pending.partition(b"\n")
                groups.append(int(group))
        tuner.send_signal(signal.SIGTERM)
        stdout, stderr = tuner.communicate(timeout=10)
        observations = [(group, subprocess.run(["pgrep", "-g", str(group)], capture_output=True, text=True)) for group in groups]
        assert tuner.returncode == 143, (tuner.returncode, stdout, stderr, [(group, p.stdout) for group, p in observations])
        for group, survivors in observations:
            assert survivors.returncode == 1 and not survivors.stdout, (group, survivors.stdout)
    finally:
        os.close(ready_fd)
        if tuner.poll() is None:
            tuner.kill()
        tuner.wait()
        for group in groups:
            try:
                os.killpg(group, signal.SIGKILL)
            except ProcessLookupError:
                pass
            while True:
                try:
                    os.waitpid(-group, 0)
                except ChildProcessError:
                    break
    print("ok: SIGTERM tuner exits 143 and leaves zero pgrep -g survivors in both registered run groups")

    assert replay.run_check(f"GROUP_EXIT=1 {sys.executable} {group_fixture}", work, 30)
    group = int(group_file.read_text())
    try:
        survivors = subprocess.run(["pgrep", "-g", str(group)], capture_output=True, text=True)
        assert survivors.returncode == 1 and not survivors.stdout, survivors.stdout
    finally:
        try:
            os.killpg(group, signal.SIGKILL)
        except ProcessLookupError:
            pass
        while True:
            try:
                os.waitpid(-group, 0)
            except ChildProcessError:
                break

    coding = op / "replay" / "coding"
    coding.mkdir()
    task = coding / "t.json"
    task.write_text(json.dumps({"id": "t", "prompt": "p", "policy": "../../AGENTS.md",
                                "detector": {"kind": "command", "command": "grep -q done out.txt"}}))
    (fake / "claude").write_text("#!/bin/sh\n[ \"$1\" = --version ] && { echo 9.9; exit 0; }\n"
                                 f"echo \"$@\" > {tmp}/argv\n"
                                 f"echo \"${{CLAUDE_CODE_DISABLE_ADVISOR_TOOL:-}}\" > {tmp}/advisor_off\n"
                                 'while [ $# -gt 0 ]; do [ "$1" = --debug-file ] && [ -n "$STUB_DEBUG" ] && cp "$STUB_DEBUG" "$2"; shift; done\n'
                                 "echo done > out.txt\n"
                                 "echo '{\"type\":\"result\",\"subtype\":\"success\",\"result\":\"ok\",\"model\":\"m\"}'\n")
    (tmp / "attached.log").write_text(debug_log)
    (tmp / "detached.log").write_text(detached_log)
    arm = run(task, "--arm", "full", "--model", "haiku", "--effort", "low", "--advisor", "opus", "--out", tmp / "advisor-x.jsonl",
              env=dict(env, STUB_DEBUG=str(tmp / "attached.log")))
    assert arm.returncode == 0, arm.stderr[-2000:]
    row = json.loads((tmp / "advisor-x.jsonl").read_text())
    argv = (tmp / "argv").read_text()
    assert "--model haiku" in argv and "--effort low" in argv and "--advisor opus" in argv and "--strict-mcp-config" in argv, argv
    assert row["advisor_attached"] is True and row["advisor_model_resolved"] == "claude-opus-5-5", row
    assert row["pass"] is True and (row["model"], row["effort"], row["advisor"], row["advisor_calls"]) == ("haiku", "low", "opus", 0), row
    assert row["usd"] == 0 and row["usd_basis"].startswith("total_cost_usd"), row
    detached = run(task, "--arm", "full", "--model", "haiku", "--advisor", "opus", "--out", tmp / "advisor-d.jsonl",
                   env=dict(env, STUB_DEBUG=str(tmp / "detached.log")))
    row = json.loads((tmp / "advisor-d.jsonl").read_text())
    assert row["pass"] is None and row["advisor_attached"] is False and "not attached" in row["error"], row
    assert (tmp / "advisor_off").read_text().strip() == "", "an advisor arm must not disable the advisor"
    control = run(task, "--arm", "full", "--model", "haiku", "--out", tmp / "advisor-c.jsonl",
                  env=dict(env, STUB_DEBUG=str(tmp / "detached.log")))
    assert control.returncode == 0, control.stderr[-2000:]
    assert (tmp / "advisor_off").read_text().strip() == "1", "a control arm must set CLAUDE_CODE_DISABLE_ADVISOR_TOOL"
    assert json.loads((tmp / "advisor-c.jsonl").read_text())["pass"] is True
    leaked = run(task, "--arm", "full", "--model", "haiku", "--out", tmp / "advisor-l.jsonl",
                 env=dict(env, STUB_DEBUG=str(tmp / "attached.log")))
    row = json.loads((tmp / "advisor-l.jsonl").read_text())
    assert row["pass"] is None and "control run had an advisor" in row["error"], row
    fable = run(task, "--arm", "full", "--advisor", "fable", "--out", tmp / "advisor-y.jsonl", env=env)
    assert fable.returncode != 0 and "Fable" in fable.stderr, fable
    print("ok: a replay/coding scenario with its own policy runs --model/--effort/--advisor --strict-mcp-config, scores through its command, and nulls a detached advisor run")
