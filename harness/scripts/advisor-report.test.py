#!/usr/bin/env python3
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile

SCRIPT = Path(__file__).resolve().parent / "advisor-report.py"
spec = importlib.util.spec_from_file_location("advisor_report", SCRIPT)
report = importlib.util.module_from_spec(spec)
spec.loader.exec_module(report)
from policy_stats import paired_diff, wilson


def rows(passes: dict[str, list[bool | None]], usd: float, **extra) -> list[dict]:
    return [{"scenario": s, "arm": "", "trial": t, "model": "opus", "effort": "medium", "advisor": None, "advisor_calls": 0,
             "advisor_usd": 0.0, "usd": usd, "wall_s": 10.0 + t, "pass": p, "tokens": 0, **extra}
            for s, ps in passes.items() for t, p in enumerate(ps, 1)]


incumbent = rows({"a": [True, False], "b": [True, True], "c": [False, None]}, 2.0)
challenger = rows({"a": [True, True], "b": [True, True], "c": [False, True]}, 1.0, advisor="opus", advisor_calls=1, advisor_usd=0.25)
with tempfile.TemporaryDirectory() as tmp:
    for name, rs in (("opus", incumbent), ("haiku-adv", challenger)):
        (Path(tmp) / f"{name}.jsonl").write_text("".join(json.dumps(r) + "\n" for r in rs))
    argv = [sys.executable, str(SCRIPT), f"{tmp}/opus.jsonl", f"{tmp}/haiku-adv.jsonl", "--incumbent", "opus"]
    first, second = (subprocess.run(argv, capture_output=True, text=True, check=True).stdout for _ in range(2))
assert first == second, (first, second)
stats, diffs, verdicts = report.report({"opus": incumbent, "haiku-adv": challenger}, "opus", {})
assert stats["opus"]["wilson"] == wilson(3 / 5, 5) and stats["haiku-adv"]["wilson"] == wilson(5 / 6, 6), stats
assert diffs["haiku-adv"] == paired_diff(challenger, incumbent, seed=0, b=4000), diffs
assert diffs["haiku-adv"][0] == (0.5 + 0 + 0.5) / 3, diffs
s = stats["haiku-adv"]
assert (s["n"], s["list_run"], s["list_success"]) == (6, 1.0, 6 / 5), s
assert abs(s["plan_run"] - 0.052) < 1e-12 and (s["wall_median"], s["advisor_mean"], s["advisor_share_runs"], s["advisor_usd_share"]) == (11.5, 1, 1, 0.25), s
assert "haiku-adv: " + verdicts["haiku-adv"] + " vs opus" in first and f"{100 * diffs['haiku-adv'][1]:+.1f}" in first, first
print("ok: 2 arms x 3 tasks x 2 trials: Wilson equals wilson(), the paired CI repeats under seed 0, costs and advisor use per run")

codex = rows({"a": [True, True], "b": [True, False]}, None, provider="codex")
c = report.summarize(codex, 6.0)
assert (c["list_run"], c["plan_run"], c["plan_success"]) == (None, 1.5, 2.0), c
assert report.summarize(codex)["plan_success"] is None
print("ok: codex rows spread --codex-plan-usd per run and stay unpriced without it")

four = ["a", "b", "c", "d"]
cases = {
    "BEATS": (rows({s: [False] * 5 for s in four}, 1.0), rows({s: [True] * 5 for s in four}, 1.0)),
    "MATCHES-FOR-LESS": (rows({s: [True, True] for s in four}, 2.0), rows({s: [True, True] for s in four}, 1.0)),
    "NO-GAIN": (rows({s: [True, True] for s in four}, 1.0), rows({s: [True, True] for s in four}, 2.0)),
    "UNDECIDED": (rows({"a": [True], "b": [False]}, 1.0), rows({"a": [False], "b": [True]}, 1.0)),
}
for expected, (inc, ch) in cases.items():
    got = report.report({"inc": inc, "ch": ch}, "inc", {})[2]["ch"]
    assert got == expected, (expected, got)
print("ok: each verdict branch: BEATS, MATCHES-FOR-LESS, NO-GAIN, UNDECIDED")
