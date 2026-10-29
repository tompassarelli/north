#!/usr/bin/env python3
import hashlib
import importlib.util
import json
import os
import sqlite3
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import tomllib

SCRIPT = Path(__file__).resolve().parent / "harness-tuner.py"
spec = importlib.util.spec_from_file_location("harness_tuner", SCRIPT)
tuner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(tuner)
replay = tuner.load_replay()


def run(*args, env=None):
    return subprocess.run([sys.executable, str(SCRIPT), *map(str, args)], env=env or os.environ, capture_output=True, text=True)


def write(path: Path, data) -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(data if isinstance(data, str) else json.dumps(data))
    return path


with tempfile.TemporaryDirectory() as tmp:
    tmp = Path(tmp)
    write(tmp / ".claude.json", {"cachedUsageUtilization":{"accountUuid":"fixture-primary"}})
    write(tmp / "orchestration.toml", '[accounts.claude]\nlabels = { "9b3a7fb5" = "a" }\n')
    os.environ["CLAUDE_CONFIG_DIR"] = str(tmp)
    os.environ["AGENTS_ORCHESTRATION"] = str(tmp / "orchestration.toml")
    sha = "0123456789abcdef0123456789abcdef01234567"
    tune = tmp / "tune"
    write(tune / "parser.json", {"id": "parser", "prompt": "Fix the  Parser\n", "detector": {"kind": "regex", "pattern": "x"}})
    write(tune / "deep/box-north-a.json", {"id": "box-north-a", "kind": "box", "commit": sha, "brief": "Land the box", "detector": {}})
    clean = tmp / "judge-clean"
    write(clean / "lexer.json", {"id": "lexer", "prompt": "Fix the lexer", "detector": {"kind": "regex", "pattern": "x"}})
    write(clean / "box-north-b.json", {"id": "box-north-b", "kind": "box", "commit": "f" * 40, "brief": "Other box", "detector": {}})
    ok = run("sets", tune, clean)
    assert ok.returncode == 0 and "2 tuning and 2 judging" in ok.stdout, ok
    shared = {
        "stem": ("parser.json", {"id": "p2", "prompt": "unrelated", "detector": {}}),
        "commit": ("box-x.json", {"id": "box-x", "kind": "box", "commit": sha.upper(), "brief": "a new brief", "detector": {}}),
        "text": ("other.json", {"id": "other", "prompt": "fix the parser", "detector": {}}),
    }
    for kind, (name, data) in shared.items():
        judge = tmp / f"judge-{kind}"
        write(judge / "lexer.json", {"id": "lexer", "prompt": "Fix the lexer", "detector": {}})
        write(judge / name, data)
        refused = run("sets", tune, judge)
        assert refused.returncode == 1 and f"{kind}:" in refused.stderr and f"judge:{name}" in refused.stderr, (kind, refused)
    empty = run("sets", tune, tmp / "nothing")
    assert empty.returncode == 1 and "0 judging" in empty.stderr, empty
    print("ok: sets passes disjoint sets and refuses, naming the ids, a shared file stem, box commit or normalized prompt, and an empty set")

    repo = tmp / "repo"
    skill = write(repo / "harness/skills/demo/SKILL.md", "---\nname: demo\n---\n# Demo\n\n- Run the tests once.\n- Report the diff.\n")
    write(repo / "harness/hooks/nag.sh", "#!/bin/sh\nset -eu\necho 'land what passes and hand off the rest'\nexit 0\n")
    write(repo / "harness/hooks/nag.test.sh", "echo 'land what passes'\n")
    runs = write(tmp / "tuning-runs.jsonl", "".join(json.dumps(r) + "\n" for r in [
        {"scenario": "parser", "arm": "full", "trial": 1, "pass": False, "wall_s": 30.0, "turns": 9, "final_text": "gave up on the parser"},
        {"scenario": "lexer", "arm": "full", "trial": 1, "pass": None, "wall_s": 5.0, "error": "no result (exit 1): boom"},
        {"scenario": "slowpoke", "arm": "full", "trial": 1, "pass": True, "wall_s": 900.0, "turns": 80, "tools_called": ["Bash"]},
        {"scenario": "quick", "arm": "full", "trial": 1, "pass": True, "wall_s": 10.0, "turns": 3},
    ]))
    brief = tmp / "brief.md"
    proposed = run("propose", "--runs", runs, "--surface", "skill", "--target", "harness/skills/demo/SKILL.md", "--out", brief, "--repo", repo)
    assert proposed.returncode == 0, proposed.stderr
    text = brief.read_text()
    failed_part, slow_part = text.split("### Slow")[0], text.split("### Slow")[1].split("## Current")[0]
    assert "gave up on the parser" in failed_part and "boom" in failed_part and "slowpoke" in slow_part and "quick" not in slow_part, text
    assert "- Run the tests once." in text and "at most 5 edits" in text and "adoption rule" in text, text
    answer_schema = json.loads(brief.with_suffix(".schema.json").read_text())
    assert answer_schema["properties"]["edits"]["maxItems"] == 5 and "may_break" in answer_schema["properties"]["edits"]["items"]["required"]
    graded = run("propose", "--runs", runs, "--surface", "hook-message", "--target", "harness/hooks/nag.test.sh", "--out", brief, "--repo", repo)
    assert graded.returncode == 1 and "grader" in graded.stderr, graded
    print("ok: propose briefs failed, errored and p75-slow runs with the target text, the rules and a 5-edit schema, and refuses a test file")

    def edit(i, **kw):
        return {"id": f"e{i}", "surface": "skill", "target": "harness/skills/demo/SKILL.md", "old": "- Run the tests once.\n",
                "new": f"- Run the tests once; EDITED {i}.\n", "fixes": "parser give-ups", "may_break": "slow passes", **kw}

    state = tmp / "state/harness-tuner-2026-10-12.jsonl"
    for bad, why in (({"edits": [edit(i) for i in range(1, 7)]}, "1 to 5 edits"),
                     ({"edits": [edit(1, may_break="")]}, "may_break"),
                     ({"edits": [edit(1, old="- Missing.\n")]}, "occurs 0 times"),
                     ({"edits": [edit(1, surface="hook-message", target="harness/hooks/nag.test.sh")]}, "grader")):
        refused = run("record", write(tmp / "bad.json", bad), "--out", state, "--repo", repo)
        assert refused.returncode == 1 and why in refused.stderr and not state.exists(), (why, refused)
    recorded = run("record", write(tmp / "answer.json", {"edits": [edit(1), edit(2)]}), "--out", state, "--repo", repo)
    assert recorded.returncode == 0 and [e["id"] for e in tuner.read_rows([state])] == ["e1", "e2"], recorded
    again = run("record", tmp / "answer.json", "--out", state, "--repo", repo)
    assert again.returncode == 1 and "runs the proposer once" in again.stderr, again
    print("ok: record refuses 6 edits, a missing may-break line, an old text not found once and a grader target; writes the jsonl once")

    before = hashlib.sha256(skill.read_bytes()).hexdigest()
    base = write(tmp / "AGENTS.md", "# Policy\n\n- Keep it short.\n")
    variants = tmp / "variants"
    applied = run("apply", state, "--id", "e1", "--out", variants, "--base", base, "--repo", repo)
    assert applied.returncode == 0, applied.stderr
    assert hashlib.sha256(skill.read_bytes()).hexdigest() == before, "apply touched the real surface"
    pair = {v["name"]: v for v in tomllib.loads((variants / "e1/variants.toml").read_text())["variant"]}
    edited, kept = (replay.variant_policy(base.read_text(), pair[n]) for n in ("e1", "baseline"))
    assert "EDITED 1" in edited and "- Run the tests once.\n" not in edited and "- Report the diff." in edited, edited
    assert "- Run the tests once.\n" in kept and "EDITED" not in kept and "name: demo" not in kept, kept
    assert (variants / "e1/e1.policy.md").read_text() == edited
    hooked = write(tmp / "hook.json", edit(3, surface="hook-message", target="harness/hooks/nag.sh", old="hand off the rest", new="report"))
    assert run("apply", hooked, "--out", variants, "--repo", repo).returncode == 0
    assert (variants / "e3/e3.surface.md").read_text().endswith("echo 'land what passes and report'\n")
    assert "set -eu" not in (variants / "e3/baseline.surface.md").read_text()
    print("ok: apply writes a baseline + edit variant pair policy-replay's variant_policy reads, leaves the repo file unchanged, and cuts a hook edit to its message line")

    fake = tmp / "bin"
    write(fake / "claude", "#!/bin/sh\n[ \"$1\" = --version ] && { echo 9; exit 0; }\n"
          "while [ $# -gt 0 ]; do [ \"$1\" = --append-system-prompt-file ] && policy=$2; shift; done\n"
          "if grep -q EDITED \"$policy\"; then t=ok; else t=no; fi\n"
          "echo '{\"type\":\"result\",\"subtype\":\"success\",\"result\":\"'$t'\",\"total_cost_usd\":2.0,\"usage\":{\"input_tokens\":10}}'\n").chmod(0o755)
    env = dict(os.environ, PATH=f"{fake}:{os.environ['PATH']}", NORTH_BIN=os.environ.get("NORTH_PACE_BIN") or str(SCRIPT.parents[2] / "target/debug/north"))
    scen = [write(tmp / f"judge/s{i}.json", {"id": f"s{i}", "prompt": "p", "detector": {"kind": "regex", "pattern": "^ok"}}) for i in range(4)]
    results = tmp / "results.jsonl"
    capped = run("replay", *scen, "--variants", variants / "e1", "--base", base, "--trials", 1, "--jobs", 1,
                 "--out", results, "--cap-usd", 5, env=env)
    rows = tuner.read_rows([results])
    assert capped.returncode == 3 and "cap reached: list $4.00 + reserve $0.00 + next $2.00 > cap $5.00" in capped.stderr and len(rows) == 2, (capped, rows)
    assert all(r["usd"] == 2.0 and r["usd_basis"].startswith("total_cost_usd list price") for r in rows), rows
    resumed = run("replay", *scen, "--variants", variants / "e1", "--base", base, "--trials", 1, "--jobs", 2, "--out", results, env=env)
    rows = tuner.read_rows([results])
    assert resumed.returncode == 0 and len(rows) == 8 and len({(r["scenario"], r["arm"]) for r in rows}) == 8, (resumed, rows)
    assert {r["arm"]: r["pass"] for r in rows if r["scenario"] == "s0"} == {"variant:baseline": False, "variant:e1": True}, rows
    print("ok: replay runs each variant through claude with its own policy file, prices runs at list, stops at --cap-usd with exit 3 and resumes the rest")

    for reading, expected in ((83.0, False), (84.535, False), (84.63, True), (84.64, True)):
        rejected = tuner.reservation_stop(reading, 88.0, 0.105, 32)
        assert rejected == expected, (reading, rejected)
    ledger = tmp / "ledger.db"
    subprocess.run([SCRIPT.parents[2] / "bin" / "threads", "record-priors", "fixture:1:1"], env=dict(os.environ, THREADS_DB=str(ledger)), capture_output=True, check=True)
    db = sqlite3.connect(ledger)
    db.executemany("INSERT INTO runs(agent,tier,category,ended,minutes,outcome,role,cost_usd) VALUES (?,?,?,?,?,?,?,?)",
                   [("priced", "medium", "tooling", "2026-10-12T00:00:00Z", 1, "done", "worker", 0.105),
                    ("lead", "medium", "tooling", "2026-10-12T00:00:00Z", 1, "done", "lead", 9.0),
                    ("other-tier", "high", "tooling", "2026-10-12T00:00:00Z", 1, "done", "worker", 9.0)])
    db.commit()
    db.close()
    write(tmp / "costs.toml", '[[service]]\nid="claude-a"\nkind="ai-plan"\nusd_per_week=100\n')
    write(fake / "claude", "#!/bin/sh\n[ \"$1\" = --version ] && { echo 9; exit 0; }\n"
          "echo '{\"type\":\"rate_limit_event\",\"rate_limit_info\":{\"unifiedWindows\":{\"seven_day\":{\"utilization\":0.83}}}}'\n"
          "echo '{\"type\":\"result\",\"subtype\":\"success\",\"result\":\"ok\",\"model\":\"opus\"}'\n").chmod(0o755)
    reserved = write(tmp / "reserved.jsonl", json.dumps({"scenario": "seed", "arm": "variant:baseline", "trial": 1,
                                                       "pass": True, "usd": 0, "seven_day_pct": 83}) + "\n")
    weekly = run("replay", scen[0], "--variants", variants / "e1", "--base", base,
                 "--trials", 40, "--jobs", 32, "--out", reserved, "--cap-week-pct", 5,
                 env=dict(env, THREADS_DB=str(ledger), AGENTS_OPERATOR_DIR=str(tmp)))
    charged = len(tuner.read_rows([reserved])) - 1
    assert weekly.returncode == 3 and charged == 47 and 83 + charged * 0.105 <= 88, (weekly, charged)
    assert "reserve" in weekly.stderr and "next 0.105" in weekly.stderr, weekly.stderr
    retried = run("replay", scen[0], "--variants", variants / "e1", "--base", base,
                  "--trials", 40, "--jobs", 32, "--out", reserved, "--cap-week-pct", 5,
                  env=dict(env, THREADS_DB=str(ledger), AGENTS_OPERATOR_DIR=str(tmp)))
    assert retried.returncode == 3 and "replayed 0 of" in retried.stdout, retried
    assert len(tuner.read_rows([reserved])) - 1 == charged
    snapshot_home = tmp / "snapshot-home"
    write(snapshot_home / ".config/threads/remote", "fixture\n")
    write(fake / "ssh", "#!/bin/sh\n[ -n \"$SNAPSHOT_FIXTURE\" ] && THREADS_DB=\"$SNAPSHOT_FIXTURE\" exec \"$NORTH_BIN\" threads snapshot-pages || exit 1\n").chmod(0o755)
    snapshot_env = dict(env, HOME=str(snapshot_home), AGENTS_OPERATOR_DIR=str(tmp), NORTH_BIN=os.environ.get("NORTH_PACE_BIN") or str(SCRIPT.parents[2] / "target/debug/north"))
    snapshot_env.pop("THREADS_DB", None)
    snapshot_args = ("replay", scen[0], "--variants", variants / "e1", "--base", base,
                     "--out", tmp / "snapshot-refused.jsonl", "--cap-week-pct", 5)
    for present in (False, True):
        if present:
            snapshot = write(snapshot_home / ".local/state/threads/snapshot.db", "")
            snapshot.write_bytes(ledger.read_bytes())
            os.utime(snapshot, (0, 0))
        refused_snapshot = run(*snapshot_args, env=snapshot_env)
        assert refused_snapshot.returncode == 3, refused_snapshot
        assert refused_snapshot.stderr.strip() == "harness-tuner: cap reached: seven-day reservation unavailable: NorthThreads snapshot missing or stale", refused_snapshot
        assert not (tmp / "snapshot-refused.jsonl").exists()
    import usage
    db = sqlite3.connect(ledger)
    usage.record(db, [{"ts": "2026-10-12T00:00:00Z", "provider": "claude", "account": usage.local_account("claude") or "", "kind": "window",
                       "name": "week", "used_pct": 83}])
    db.close()
    refreshed_out = tmp / "snapshot-refreshed.jsonl"; Path(f"{snapshot}.tried").unlink()
    refreshed = run("replay", scen[0], "--variants", variants / "e1", "--base", base,
                    "--out", refreshed_out, "--cap-week-pct", .2,
                    env=dict(snapshot_env, SNAPSHOT_FIXTURE=str(ledger)))
    assert refreshed_out.exists(), refreshed
    assert refreshed.returncode == 3 and len(tuner.read_rows([refreshed_out])) == 1, refreshed
    assert list(sqlite3.connect(snapshot).iterdump()) == list(sqlite3.connect(ledger).iterdump()) and snapshot.stat().st_mtime > 0
    print("ok: a stale reservation snapshot refreshes through the public SSH snapshot command before admission")
    print("ok: north#50 83%/0.105 points/32 in flight replay reserves 3.36 points, admits 47 runs under delayed readings and ends at 87.935%; 84.63/84.64 cannot reserve a 33rd run")

    unpriced = tmp / "unpriced.jsonl"
    write(unpriced, json.dumps({"scenario": "s0", "arm": "variant:baseline", "trial": 1, "pass": True, "model": "unpriced"}) + "\n")
    refused = run("replay", scen[0], "--variants", variants / "e1", "--base", base,
                  "--trials", 2, "--jobs", 2, "--out", unpriced, "--cap-usd", 5, env=env)
    assert refused.returncode == 3 and len(tuner.read_rows([unpriced])) == 1, refused
    assert "harness-tuner: cap reached: unpriced run s0" in refused.stderr.splitlines(), refused.stderr
    unpriced_control = run("replay", scen[0], "--variants", variants / "e1", "--base", base,
                          "--trials", 2, "--jobs", 2, "--out", unpriced, env=env)
    assert unpriced_control.returncode == 0 and len(tuner.read_rows([unpriced])) == 4, unpriced_control
    print("ok: --cap-usd refuses further admissions after one unpriced run with the exact diagnostic; uncapped replay resumes all remaining runs")

    judged = run("judge", results, "--edits", state, "--cap-usd", 20)
    assert judged.returncode == 0, judged.stderr
    assert "e1: BEATS vs baseline; fixes parser give-ups; may break slow passes" in judged.stdout, judged.stdout
    assert "+100.0 [+100.0, +100.0]" in judged.stdout and "2.0000" in judged.stdout and "inf" in judged.stdout, judged.stdout
    assert "wins 1 (e1)" in judged.stdout and "total list spend $16.00 of cap $20.00" in judged.stdout, judged.stdout
    s = lambda rate, n, cost: {"wilson": replay.wilson(rate, n), "list_success": cost}
    for expected, edit_s, base_s, diff in (("MATCHES-FOR-LESS", s(0.9, 10, 1.0), s(0.9, 10, 2.0), (0.0, -0.05, 0.05)),
                                           ("NO-GAIN", s(0.9, 10, 3.0), s(0.9, 10, 2.0), (0.0, -0.05, 0.05)),
                                           ("UNDECIDED", s(0.9, 10, 1.0), s(0.9, 10, 2.0), (0.0, -0.3, 0.3))):
        assert tuner.verdict(edit_s, base_s, diff, 0.1) == expected, expected
    print("ok: judge reports the paired difference, list $/success, median wall and the north#36 verdict per edit, spend against the cap")
