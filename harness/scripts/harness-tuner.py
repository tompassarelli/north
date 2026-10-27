#!/usr/bin/env python3
"""One bounded harness-learning trial (north#47): check the tuning and judging sets are disjoint, brief one proposer, record and materialize its edits as policy-replay variants, replay them under a spend cap and judge each against the baseline by the north#36 rule."""

from __future__ import annotations

import argparse
from concurrent.futures import FIRST_COMPLETED, ThreadPoolExecutor, wait
from datetime import datetime, timezone
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import statistics
import sys
import tempfile
import tomllib

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(ROOT / "harness/lib"))
from defaults import DEFAULTS  # noqa: E402
from policy_stats import paired_diff, wilson  # noqa: E402

STATE = Path.home() / ".local/state/agents/replay"
MAX_EDITS = 5
BASELINE = "baseline"
SURFACES = {
    "skill": (re.compile(r"^(harness|agent-machinery)/skills/.+\.md$"),),
    "prompt": (re.compile(r"^harness/claude/agents/[^/]+\.md$"), re.compile(r"^harness/codex/prompts/[^/]+\.md$"),
               re.compile(r"^harness/skills/workers/references/[^/]+\.md$")),
    "hook-message": (re.compile(r"^harness/hooks/[^/]+\.sh$"), re.compile(r"^harness/hooks/lib/[^/]+\.py$")),
}
OUT_OF_REACH = re.compile(r"\.test\.|(^|/)(fixtures|replay)/|policy-replay|policy_stats|advisor-report|harness-tuner"
                          r"|defaults\.toml$|operating-point")
EDIT_ID = re.compile(r"^e[1-9][0-9]*$")
RULES = f"""- Propose at most {MAX_EDITS} edits, all to the one target file named below.
- Each edit replaces one exact `old` string that occurs once in the target with `new`; keep it small.
- Each edit names the failure it fixes (`fixes`) and what it may break (`may_break`), one line each.
- Never edit graders (the scenarios' tests, detectors or scoring scripts), the replay sets or the adoption rule (north#36:
  an edit wins only if it beats the current harness on the held-out judging set within the margin, on cost and success).
- You see only the tuning set; the judging set is held out and decides.
- Answer with one JSON object that follows the schema below and nothing else."""


def load_replay():
    spec = importlib.util.spec_from_file_location("policy_replay", HERE / "policy-replay.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def load_threads():
    spec = importlib.util.spec_from_file_location("defaults_measure", HERE / "defaults-measure.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.T


def normalized(text: str) -> str:
    return hashlib.sha256(" ".join(text.lower().split()).encode()).hexdigest()[:16]


def task_ids(path: Path) -> set[str]:
    data = json.loads(path.read_text())
    ids = {f"stem:{path.stem}"}
    if data.get("kind") == "box" and data.get("commit"):
        ids.add(f"commit:{data['commit'].lower()}")
    text = data.get("brief") if data.get("kind") == "box" else data.get("prompt")
    if isinstance(text, str) and text.strip():
        ids.add(f"text:{normalized(text)}")
    return ids


def scenario_files(root: Path) -> list[Path]:
    return sorted(p for p in root.rglob("*.json") if p.is_file())


def overlap(tune: Path, judge: Path) -> tuple[dict[str, list[str]], int, int]:
    owners: dict[str, list[str]] = {}
    tuning, judging = scenario_files(tune), scenario_files(judge)
    for side, files in (("tune", tuning), ("judge", judging)):
        for path in files:
            for tid in task_ids(path):
                owners.setdefault(tid, []).append(f"{side}:{path.name}")
    shared = {tid: names for tid, names in owners.items()
              if any(n.startswith("tune:") for n in names) and any(n.startswith("judge:") for n in names)}
    return shared, len(tuning), len(judging)


def cmd_sets(args) -> int:
    shared, n_tune, n_judge = overlap(args.tune, args.judge)
    if not n_tune or not n_judge:
        print(f"harness-tuner: refuse: {n_tune} tuning and {n_judge} judging scenarios; both sets need at least one", file=sys.stderr)
        return 1
    if shared:
        print(f"harness-tuner: refuse: the tuning and judging sets share {len(shared)} task ids:", file=sys.stderr)
        for tid, names in sorted(shared.items()):
            print(f"  {tid}  {' '.join(sorted(names))}", file=sys.stderr)
        return 1
    print(f"ok: {n_tune} tuning and {n_judge} judging scenarios share no task id (stem, box commit, normalized prompt or brief)")
    return 0


def target_path(repo: Path, rel: str, surface: str) -> Path:
    path = (repo / rel).resolve()
    try:
        rel = str(path.relative_to(repo.resolve()))
    except ValueError:
        raise SystemExit(f"harness-tuner: refuse: target {rel} is outside {repo}")
    if OUT_OF_REACH.search(rel):
        raise SystemExit(f"harness-tuner: refuse: target {rel} is a grader, replay set or adoption rule")
    if not any(p.match(rel) for p in SURFACES[surface]):
        raise SystemExit(f"harness-tuner: refuse: target {rel} is not a {surface} surface")
    if not path.is_file():
        raise SystemExit(f"harness-tuner: refuse: target {rel} does not exist")
    return path


def schema(surface: str, target: str) -> dict:
    line = {"type": "string", "minLength": 1, "pattern": "^[^\\n]+$"}
    edit = {"type": "object", "additionalProperties": False,
            "required": ["id", "surface", "target", "old", "new", "fixes", "may_break"],
            "properties": {"id": {"type": "string", "pattern": EDIT_ID.pattern}, "surface": {"const": surface},
                           "target": {"const": target}, "old": {"type": "string", "minLength": 1}, "new": {"type": "string"},
                           "fixes": line, "may_break": line}}
    return {"$schema": "https://json-schema.org/draft/2020-12/schema", "type": "object", "additionalProperties": False,
            "required": ["edits"], "properties": {"edits": {"type": "array", "minItems": 1, "maxItems": MAX_EDITS, "items": edit}}}


def read_rows(paths: list[Path]) -> list[dict]:
    return [json.loads(line) for path in paths for line in path.read_text().splitlines() if line.strip()]


def run_summary(rows: list[dict], limit: int) -> tuple[list[str], list[str]]:
    walls = sorted(r["wall_s"] for r in rows if r.get("pass") is not None and r.get("wall_s") is not None)
    cut = walls[int(0.75 * (len(walls) - 1))] if walls else float("inf")
    tail = lambda r: " ".join(str(r.get("error") or r.get("final_text") or r.get("first_line") or "").split())[-400:]
    failed = [f"- {r['scenario']} trial {r.get('trial')} ({r.get('arm')}): "
              f"{'error' if r.get('pass') is None else 'fail'}, {r.get('wall_s')}s, {r.get('turns', '-')} turns: {tail(r)}"
              for r in rows if r.get("pass") is not True][:limit]
    slow = [f"- {r['scenario']} trial {r.get('trial')} ({r.get('arm')}): pass in {r['wall_s']}s (p75 {cut}s), "
            f"{r.get('turns', '-')} turns, tools {','.join(r.get('tools_called') or [])[:200]}"
            for r in sorted(rows, key=lambda r: -(r.get("wall_s") or 0))
            if r.get("pass") is True and (r.get("wall_s") or 0) >= cut and len(walls) > 1][:limit]
    return failed, slow


def cmd_propose(args) -> int:
    target = target_path(args.repo, args.target, args.surface)
    rel = str(target.relative_to(args.repo.resolve()))
    rows = read_rows(args.runs)
    failed, slow = run_summary(rows, args.limit)
    scored = [r for r in rows if r.get("pass") is not None]
    passed = sum(1 for r in scored if r["pass"])
    answer = schema(args.surface, rel)
    brief = "\n".join([
        f"# Harness tuner proposer: {args.surface} surface `{rel}`", "",
        "You propose small edits to one North harness surface. Each edit is replayed later on a held-out judging set",
        "against the unchanged baseline; only that set decides, so propose what you expect to generalize.", "",
        "## Rules", RULES, "",
        f"## Tuning runs: {len(rows)} runs, {passed}/{len(scored)} passed, {len(rows) - len(scored)} errors", "",
        f"### Failed or errored ({len(failed)} shown)", *(failed or ["- none"]), "",
        f"### Slow passes, at or above the 75th percentile wall ({len(slow)} shown)", *(slow or ["- none"]), "",
        f"## Current text of `{rel}`", "", "````", target.read_text().rstrip("\n"), "````", "",
        "## Answer schema", "", "```json", json.dumps(answer, indent=2), "```", ""])
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(brief)
    schema_path = args.out.with_suffix(".schema.json")
    schema_path.write_text(json.dumps(answer, indent=2) + "\n")
    print(f"{args.out}: {len(failed)} failed and {len(slow)} slow runs, target {rel}; schema {schema_path}")
    return 0


def check_edit(edit: dict, repo: Path) -> Path:
    missing = [k for k in ("id", "surface", "target", "old", "new", "fixes", "may_break") if not isinstance(edit.get(k), str)]
    if missing:
        raise SystemExit(f"harness-tuner: refuse: edit {edit.get('id')!r} lacks {missing}")
    if not EDIT_ID.match(edit["id"]):
        raise SystemExit(f"harness-tuner: refuse: edit id {edit['id']!r} is not e<N>")
    for key in ("fixes", "may_break"):
        if not edit[key].strip() or "\n" in edit[key].strip():
            raise SystemExit(f"harness-tuner: refuse: edit {edit['id']} needs one {key} line")
    if edit["surface"] not in SURFACES:
        raise SystemExit(f"harness-tuner: refuse: edit {edit['id']} surface {edit['surface']!r} is not one of {sorted(SURFACES)}")
    path = target_path(repo, edit["target"], edit["surface"])
    count = path.read_text().count(edit["old"])
    if count != 1:
        raise SystemExit(f"harness-tuner: refuse: edit {edit['id']} old text occurs {count} times in {edit['target']}, not once")
    return path


def cmd_record(args) -> int:
    answer = json.loads(args.answer.read_text())
    edits = answer.get("edits") if isinstance(answer, dict) else None
    if not isinstance(edits, list) or not 1 <= len(edits) <= MAX_EDITS:
        raise SystemExit(f"harness-tuner: refuse: the answer needs 1 to {MAX_EDITS} edits, got {len(edits) if isinstance(edits, list) else edits!r}")
    for edit in edits:
        check_edit(edit, args.repo)
    if len({(e["surface"], e["target"]) for e in edits}) != 1 or len({e["id"] for e in edits}) != len(edits):
        raise SystemExit("harness-tuner: refuse: the edits need one surface, one target and distinct ids")
    out = args.out or STATE / f"harness-tuner-{datetime.now(timezone.utc):%Y-%m-%d}.jsonl"
    if out.exists():
        raise SystemExit(f"harness-tuner: refuse: {out} exists; the trial runs the proposer once")
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text("".join(json.dumps(e) + "\n" for e in edits))
    print(f"{out}: {len(edits)} edits to {edits[0]['target']}")
    for e in edits:
        print(f"  {e['id']}: fixes {e['fixes']} | may break {e['may_break']}")
    return 0


def pick_edit(path: Path, edit_id: str | None) -> dict:
    try:
        edits = [json.loads(path.read_text())]
    except json.JSONDecodeError:
        edits = read_rows([path])
    edits = [e for e in edits if edit_id is None or e.get("id") == edit_id]
    if len(edits) != 1:
        raise SystemExit(f"harness-tuner: refuse: {path} holds {len(edits)} matching edits; pass --id")
    return edits[0]


def surface_text(edit: dict, text: str, at: int, length: int) -> str:
    if edit["surface"] != "hook-message":
        return text
    start, end = text.rfind("\n", 0, at) + 1, text.find("\n", at + length)
    return f"Hook message from {edit['target']}:\n{text[start:end if end >= 0 else len(text)]}\n"


def cmd_apply(args) -> int:
    edit = pick_edit(args.edit, args.id)
    path = check_edit(edit, args.repo)
    original = path.read_text()
    edited = original.replace(edit["old"], edit["new"], 1)
    out = args.out / edit["id"]
    out.mkdir(parents=True, exist_ok=True)
    at = original.index(edit["old"])
    files = {BASELINE: surface_text(edit, original, at, len(edit["old"])), edit["id"]: surface_text(edit, edited, at, len(edit["new"]))}
    lines = []
    for name, text in files.items():
        surface = out / f"{name}.surface.md"
        surface.write_text(text)
        lines += ["[[variant]]", f"name = {json.dumps(name)}", f"plus = [{json.dumps(str(surface))}]", ""]
    (out / "variants.toml").write_text("\n".join(lines))
    (out / "edit.json").write_text(json.dumps(edit, indent=2) + "\n")
    if args.base:
        replay = load_replay()
        for variant in tomllib.loads((out / "variants.toml").read_text())["variant"]:
            (out / f"{variant['name']}.policy.md").write_text(replay.variant_policy(args.base.read_text(), variant))
    print(f"{out}/variants.toml: {BASELINE} and {edit['id']} over {edit['target']}"
          + (f"; --policy files {out}/{BASELINE}.policy.md and {out}/{edit['id']}.policy.md" if args.base else ""))
    return 0


def list_price(row: dict, catalog: dict) -> float | None:
    if row.get("usd") is not None:
        return row["usd"]
    api = next((m["value"]["api"] for m in catalog.get("models", {}).values()
                if m.get("model") == row.get("model") and isinstance(m.get("value"), dict) and m["value"].get("api")), None)
    if not api:
        return None
    cached = api.get("cached_input_per_million", api["input_per_million"] * 0.1)
    return round((row.get("input_tokens", 0) * api["input_per_million"] + row.get("cache_read_tokens", 0) * cached
                  + row.get("output_tokens", 0) * api["output_per_million"]) / 1e6, 4)




from usage import reservation_stop


def ledger_reservation(provider: str, model: str, effort: str) -> tuple[float | None, float | None]:
    from usage import ledger_reservation as measured_reservation
    return measured_reservation(provider, model, effort, threads=load_threads())


def cmd_replay(args) -> int:
    replay = load_replay()
    replay.install_signal_handlers()
    catalog = tomllib.loads((ROOT / "harness/model-catalog.toml").read_text())
    base = args.base.read_text()
    policies: dict[str, str] = {}
    for directory in args.variants:
        for variant in tomllib.loads((directory / "variants.toml").read_text())["variant"]:
            text = replay.variant_policy(base, variant)
            if policies.setdefault(variant["name"], text) != text:
                raise SystemExit(f"harness-tuner: refuse: variant {variant['name']} differs between --variants dirs")
    if BASELINE not in policies:
        raise SystemExit("harness-tuner: refuse: no baseline variant")
    scenarios = [json.loads(p.read_text()) for p in args.scenarios]
    prior = read_rows([args.out]) if args.out.exists() else []
    done = {(r["scenario"], r["arm"], r["trial"]) for r in prior if r.get("pass") is not None}
    spent = sum(list_price(r, catalog) or 0.0 for r in prior)
    weeks = [r["seven_day_pct"] for r in prior if r.get("seven_day_pct") is not None]
    plan = [(s, name, t) for s in scenarios for t in range(1, args.trials + 1) for name in policies
            if (s["id"], f"variant:{name}", t) not in done]
    pick = {"model": args.model, "effort": args.effort}
    expected_week, meter = (None, None)
    measurements = []
    if args.cap_week_pct is not None:
        try:
            estimates = [ledger_reservation("claude", args.model or s.get("model", "opus"),
                                             args.effort or s.get("effort", "medium")) for s in scenarios]
        except RuntimeError as error:
            print(error, file=sys.stderr)
            return 3
        expected_week = max(p for p, _ in estimates) if all(p is not None for p, _ in estimates) else None
        meter = estimates[0][1]
        from usage import ledger_reservation as measured_reservation
        measurements = [measured_reservation("claude",
                        args.model or s.get("model", "opus"),
                        args.effort or s.get("effort", "medium"),
                        threads=load_threads(), details=True) or {} for s in scenarios]
    starts = [r["seven_day_start_pct"] for r in prior if r.get("seven_day_start_pct") is not None]
    start_week = starts[0] if starts else weeks[0] if weeks else meter
    if meter is not None:
        weeks.append(meter)
    week_charged = max(max(weeks) - start_week if weeks and start_week is not None else 0.0,
                       sum(r.get("seven_day_reserved_points", 0.0) for r in prior))
    prices = [list_price(r, catalog) for r in prior]
    unpriced = next((r["scenario"] for r, price in zip(prior, prices) if price is None), None)
    measured_usd = [price for price in prices if price is not None]
    jobs = max(1, args.jobs)
    work = Path(tempfile.mkdtemp(prefix="harness-tuner-"))

    def one(scenario: dict, name: str, trial: int) -> dict:
        return replay.run_one(scenario, f"variant:{name}", trial, policies[name], pick)

    def capped(in_flight: int = 0, admit: bool = False) -> str | None:
        if args.cap_usd is not None:
            if unpriced is not None:
                return f"unpriced run {unpriced}"
            if spent >= args.cap_usd:
                return f"list ${spent:.2f} >= cap ${args.cap_usd:.2f}"
            expected = max(measured_usd) if measured_usd else 0.0
            if admit and reservation_stop(spent, args.cap_usd, expected, in_flight):
                return f"list ${spent:.2f} + reserve ${expected * in_flight:.2f} + next ${expected:.2f} > cap ${args.cap_usd:.2f}"
        if args.cap_week_pct is not None:
            if expected_week is None:
                return "seven-day reservation unavailable: no ledger-measured points per run"
            if not weeks or start_week is None:
                return "seven-day reservation unavailable: no starting meter reading"
            used = max(max(weeks) - start_week, week_charged)
            if used >= args.cap_week_pct:
                return f"seven-day meter rose {used:g} points >= cap {args.cap_week_pct}"
            if admit and reservation_stop(used, args.cap_week_pct, expected_week, in_flight):
                return f"seven-day meter rose {used:g} points + reserve {expected_week * in_flight:g} + next {expected_week:g} > cap {args.cap_week_pct}"
        return None

    args.out.parent.mkdir(parents=True, exist_ok=True)
    started, stop, queue = 0, None, list(plan)
    try:
        with ThreadPoolExecutor(max_workers=jobs) as pool:
            running = set()
            while queue or running:
                if args.cap_week_pct is not None and start_week is not None:
                    fresh = ledger_reservation("claude",
                            args.model or scenarios[0].get("model", "opus"),
                            args.effort or scenarios[0].get("effort", "medium"))[1]
                    if fresh is not None:
                        weeks.append(fresh)
                stop = stop or capped()
                while queue and not stop and len(running) < jobs:
                    stop = capped(len(running), admit=True)
                    if stop or (args.cap_usd is not None and not measured_usd and running):
                        break
                    running.add(pool.submit(one, *queue.pop(0)))
                    started += 1
                if not running:
                    break
                if stop and args.cap_week_pct is not None and start_week is not None:
                    used = max(max(weeks) - start_week, week_charged)
                    measurement = {"p75_s": max((m.get("p75_s") or 0 for m in measurements), default=0) or None,
                                   "p75_points": max((m.get("p75_points") or 0 for m in measurements), default=0) or None}
                    replay.soft_stop(args.cap_week_pct, used, measurement)
                if stop and args.cap_usd is not None:
                    walls = [r.get("wall_s") for r in prior if r.get("wall_s") is not None]
                    T = load_threads()
                    replay.soft_stop(args.cap_usd, spent,
                                     {"p75_s": T.quantile(walls, .75) if walls else None,
                                      "p75_points": T.quantile(measured_usd, .75) if measured_usd else None}, os.environ.get("CODEX_THREAD_ID"))
                finished, running = wait(running, timeout=1, return_when=FIRST_COMPLETED)
                for fut in finished:
                    row = fut.result()
                    row["usd"] = list_price(row, catalog)
                    if row.get("usd_basis", "").startswith("none") and row["usd"] is not None:
                        row["usd_basis"] = "list: model-catalog api prices x the run's tokens"
                    spent += row["usd"] or 0.0
                    prior.append(row)
                    if row["usd"] is not None:
                        measured_usd.append(row["usd"])
                    else:
                        unpriced = row["scenario"]
                    week_charged += expected_week or 0.0
                    if expected_week is not None:
                        row.update(seven_day_reserved_points=expected_week, seven_day_start_pct=start_week)
                    if row.get("seven_day_pct") is not None:
                        weeks.append(row["seven_day_pct"])
                    with args.out.open("a") as fh:
                        fh.write(json.dumps(row) + "\n")
                    print(json.dumps({k: row.get(k) for k in ("scenario", "arm", "trial", "pass", "usd", "wall_s")}), file=sys.stderr)
    finally:
        shutil.rmtree(work, ignore_errors=True)
    stop = stop or capped()
    print(f"replayed {started} of {len(plan)} planned runs ({len(done)} already in {args.out}); list spend ${spent:.2f}")
    if stop:
        print(f"harness-tuner: cap reached: {stop}" + ("" if stop.startswith("unpriced run ") else f"; {len(plan) - started} runs not started"), file=sys.stderr)
        return 3
    return 0


def verdict(edit: dict, base: dict, diff: tuple[float, float, float], margin: float) -> str:
    _, lo, hi = diff
    if edit["wilson"][0] > base["wilson"][1]:
        return "BEATS"
    if lo >= -margin and edit["list_success"] is not None and base["list_success"] is not None \
            and edit["list_success"] < base["list_success"]:
        return "MATCHES-FOR-LESS"
    if hi < margin:
        return "NO-GAIN"
    return "UNDECIDED"


def stats(rows: list[dict]) -> dict:
    scored = [r for r in rows if r.get("pass") is not None]
    wins = sum(1 for r in scored if r["pass"])
    priced = [r["usd"] for r in rows if r.get("usd") is not None]
    walls = [r["wall_s"] for r in rows if r.get("wall_s") is not None]
    rate = wins / len(scored) if scored else 0.0
    return {"runs": len(rows), "n": len(scored), "wins": wins, "rate": rate, "wilson": wilson(rate, len(scored)),
            "usd": sum(priced), "list_success": (sum(priced) / wins if wins else float("inf")) if priced else None,
            "wall_median": statistics.median(walls) if walls else None}


def cmd_judge(args) -> int:
    arms: dict[str, list[dict]] = {}
    for row in read_rows(args.results):
        arms.setdefault(row["arm"].removeprefix("variant:"), []).append(row)
    if BASELINE not in arms:
        raise SystemExit(f"harness-tuner: refuse: no {BASELINE} rows in {[str(p) for p in args.results]}")
    edits = {e["id"]: e for e in read_rows([args.edits])} if args.edits else {}
    margin = DEFAULTS["noninferiority_margin"] / 100
    table = {name: stats(rows) for name, rows in arms.items()}
    money = lambda x: "-" if x is None else "inf" if x == float("inf") else f"{x:.4f}"
    head = ("arm", "n", "pass", "wilson95", f"diff vs {BASELINE} [95%]", "list$/win", "wall_med", "verdict")
    lines, verdicts = [head], {}
    for name in [BASELINE, *sorted(n for n in arms if n != BASELINE)]:
        s = table[name]
        cells = [name, str(s["n"]), f"{100 * s['rate']:.1f}%", f"[{100 * s['wilson'][0]:.1f}%, {100 * s['wilson'][1]:.1f}%]"]
        if name == BASELINE:
            cells += ["baseline"]
        else:
            d = paired_diff(arms[name], arms[BASELINE])
            verdicts[name] = verdict(s, table[BASELINE], d, margin)
            cells += [f"{100 * d[0]:+.1f} [{100 * d[1]:+.1f}, {100 * d[2]:+.1f}]"]
        cells += [money(s["list_success"]), "-" if s["wall_median"] is None else f"{s['wall_median']:.1f}", verdicts.get(name, "-")]
        lines.append(tuple(cells))
    widths = [max(len(row[i]) for row in lines) for i in range(len(head))]
    print("\n".join("  ".join(c.ljust(w) for c, w in zip(row, widths)).rstrip() for row in lines))
    for name, v in verdicts.items():
        e = edits.get(name, {})
        print(f"{name}: {v} vs {BASELINE}" + (f"; fixes {e['fixes']}; may break {e['may_break']}" if e else ""))
    wins = [n for n, v in verdicts.items() if v in ("BEATS", "MATCHES-FOR-LESS")]
    open_ = [n for n, v in verdicts.items() if v == "UNDECIDED"]
    spend = sum(s["usd"] for s in table.values())
    print(f"edits tried {len(verdicts)}, wins {len(wins)} ({', '.join(wins) or 'none'}), losses {len(verdicts) - len(wins) - len(open_)}, "
          f"undecided {len(open_)}; total list spend ${spend:.2f}" + (f" of cap ${args.cap_usd:.2f}" if args.cap_usd is not None else ""))
    print("second round: " + ("worth it: " + ", ".join(wins + open_) + " won or stayed undecided" if wins or open_
                              else "not worth it: every edit was NO-GAIN on the judging set"))
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("sets", help="refuse when the tuning and judging scenario sets share a task id")
    p.add_argument("tune", type=Path)
    p.add_argument("judge", type=Path)
    p = sub.add_parser("propose", help="write the proposer's brief and answer schema from tuning runs")
    p.add_argument("--runs", type=Path, action="append", required=True, help="policy-replay results jsonl on the tuning set; repeatable")
    p.add_argument("--surface", choices=sorted(SURFACES), required=True)
    p.add_argument("--target", required=True, help="the surface file, relative to --repo")
    p.add_argument("--out", type=Path, required=True)
    p.add_argument("--limit", type=int, default=40, help="failed and slow runs shown, each")
    p = sub.add_parser("record", help="validate the proposer's answer and write it as harness-tuner-<UTC date>.jsonl")
    p.add_argument("answer", type=Path)
    p.add_argument("--out", type=Path)
    p = sub.add_parser("apply", help="materialize one edit as a baseline + edit policy-replay variant pair")
    p.add_argument("edit", type=Path, help="one edit object, or the recorded jsonl with --id")
    p.add_argument("--id")
    p.add_argument("--out", type=Path, default=STATE / "harness-tuner-variants")
    p.add_argument("--base", type=Path, help="base policy; also write full --policy files")
    p = sub.add_parser("replay", help="replay baseline and edits on the judging set until done or the cap; exit 3 at the cap")
    p.add_argument("scenarios", type=Path, nargs="+")
    p.add_argument("--variants", type=Path, action="append", required=True, help="an apply output dir; repeatable")
    p.add_argument("--base", type=Path, required=True, help="base policy the surfaces are appended to")
    p.add_argument("--model")
    p.add_argument("--effort", choices=["low", "medium", "high", "xhigh"])
    p.add_argument("--trials", type=int, default=3)
    p.add_argument("--jobs", type=int, default=4)
    p.add_argument("--out", type=Path, required=True, help="results jsonl, appended; finished runs in it are skipped and count toward the cap")
    p.add_argument("--cap-usd", type=float, help="reserve measured list $ for running and next runs; stop admissions before this cap")
    p.add_argument("--cap-week-pct", type=float, help="reserve ledger-measured points per run; stop admissions before the seven-day meter rises this many points")
    p = sub.add_parser("judge", help="per edit: paired difference, list $/success, median wall and the north#36 verdict")
    p.add_argument("results", type=Path, nargs="+")
    p.add_argument("--edits", type=Path, help="the recorded edits jsonl, for the fixes and may-break lines")
    p.add_argument("--cap-usd", type=float)
    for name in ("propose", "record", "apply"):
        sub.choices[name].add_argument("--repo", type=Path, default=ROOT)
    args = ap.parse_args()
    return {"sets": cmd_sets, "propose": cmd_propose, "record": cmd_record, "apply": cmd_apply,
            "replay": cmd_replay, "judge": cmd_judge}[args.cmd](args)


if __name__ == "__main__":
    sys.exit(main())
