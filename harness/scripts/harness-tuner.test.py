#!/usr/bin/env python3
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
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

    fake, home = tmp / "bin", tmp / "codex-home"
    write(home / "AGENTS.md", "live\n")
    write(home / "auth.json", "{}")
    write(fake / "codex", "#!/bin/sh\n[ \"$1\" = --version ] && { echo codex 9; exit 0; }\n"
          "test -f \"$CODEX_HOME/auth.json\" || exit 1\n"
          "if grep -q EDITED \"$CODEX_HOME/AGENTS.md\"; then t=ok; else t=no; fi\n"
          "echo '{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"'$t'\"}}'\n"
          "echo '{\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":1000000,\"cached_input_tokens\":0,\"output_tokens\":0}}'\n").chmod(0o755)
    env = dict(os.environ, PATH=f"{fake}:{os.environ['PATH']}", NORTH_CODEX_HOME=str(home))
    scen = [write(tmp / f"judge/s{i}.json", {"id": f"s{i}", "prompt": "p", "detector": {"kind": "regex", "pattern": "^ok"}}) for i in range(4)]
    results = tmp / "results.jsonl"
    capped = run("replay", *scen, "--variants", variants / "e1", "--base", base, "--trials", 1, "--jobs", 1,
                 "--out", results, "--cap-usd", 5, env=env)
    rows = tuner.read_rows([results])
    assert capped.returncode == 3 and "cap reached: list $6.00 >= cap $5.00" in capped.stderr and len(rows) == 3, (capped, rows)
    assert all(r["usd"] == 2.0 and r["usd_basis"].startswith("list") for r in rows), rows
    assert (home / "AGENTS.md").read_text() == "live\n"
    resumed = run("replay", *scen, "--variants", variants / "e1", "--base", base, "--trials", 1, "--jobs", 2, "--out", results, env=env)
    rows = tuner.read_rows([results])
    assert resumed.returncode == 0 and len(rows) == 8 and len({(r["scenario"], r["arm"]) for r in rows}) == 8, (resumed, rows)
    assert {r["arm"]: r["pass"] for r in rows if r["scenario"] == "s0"} == {"variant:baseline": False, "variant:e1": True}, rows
    print("ok: replay runs each variant through codex with its own home, prices runs at list, stops at --cap-usd with exit 3 and resumes the rest")

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
