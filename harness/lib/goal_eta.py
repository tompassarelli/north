import json
import math
import os
from pathlib import Path
import re
import sqlite3
import subprocess
import sys
from datetime import datetime, timedelta, timezone

LIB = Path(__file__).resolve().parent
sys.path.insert(0, str(LIB))
from calibration import open_ledger, threads_call
import ledger_context

BOX = re.compile(r"^\s*[-*] \[([ xX])\]\s+(.+)$", re.M)
NUMBERED = re.compile(r"^\s*\d+\.\s+(.+)$", re.M)
DONE_WHEN = re.compile(r"^#*\s*Done when:?\s*$", re.M | re.I)
DEPENDS = re.compile(r"\b(?:depends on|after)\s+(?:box(?:es)?\s+(\d+)|(?:([\w.-]+))?#(\d+))", re.I)


def stamp(value):
    result = value if isinstance(value, datetime) else datetime.fromisoformat(str(value).replace("Z", "+00:00"))
    return result if result.tzinfo else result.replace(tzinfo=timezone.utc)


def iso(value):
    return value.astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def span(minutes):
    minutes = math.ceil(minutes)
    return f"{minutes}m" if minutes < 60 else f"{minutes // 60}h{minutes % 60:02d}m"


def parse_boxes(project, issues):
    """Open Done-when boxes of the goal's issues: {id: {text, facets, deps}}; deps from `depends on`/`after` lines
    in a box (box N of the same issue, or [repo]#N for every open box of that issue) and from GitHub blockedBy."""
    boxes, numbers = {}, {int(i["number"]) for i in issues}
    for issue in issues:
        body = issue.get("body") or ""
        match = DONE_WHEN.search(body)
        section = body[match.end():] if match else body
        found = [(mark, text) for mark, text in BOX.findall(section)]
        if not found and match:
            found = [(" ", text) for text in NUMBERED.findall(section.split("\n\n", 1)[0] if section.strip() else "")]
        for index, (mark, text) in enumerate(found, 1):
            if mark != " ":
                continue
            boxes[f"{project}#{issue['number']}/{index}"] = {
                "item": int(issue["number"]), "index": index, "text": text, "deps": set(),
                "facets": "\n".join([text, body, *(f"Label: {l['name']}" for l in issue.get("labels") or [])]),
                "blocked_by": {int(b["number"]) for b in (issue.get("blockedBy") or {}).get("nodes", [])
                               if str(b.get("state", "OPEN")).upper() == "OPEN"}}
    for key, box in boxes.items():
        for local, repo, number in DEPENDS.findall(box["text"]):
            if local:
                box["deps"].add(f"{project}#{box['item']}/{local}")
            elif (repo or project) == project and int(number) in numbers:
                box["deps"] |= {k for k, b in boxes.items() if b["item"] == int(number)}
        for number in box.pop("blocked_by"):
            box["deps"] |= {k for k, b in boxes.items() if b["item"] == number}
        box["deps"] = {d for d in box["deps"] if d in boxes and d != key}
    return boxes


def box_durations(facets, rows=None, db_path=None):
    """The box ledger measures for each facet text (north threads box-eta): the box-level p75 minutes (first staffed to last
    landed over an item's chained runs) for its category, else over every category, and the box calibration scale; the
    chains come from `rows` when given, else from the ledger at `db_path`, which also gives the scale."""
    request = {"facets": facets} if rows is None else {"facets": facets, "rows": rows}
    return threads_call("box-eta", request, db=db_path)


def box_eta(measured, scale, fallback):
    """(minutes, n, source): the measured box p75 scaled by the box calibration; with no chains, the project median
    brief-to-landed."""
    eta, n, used = measured["minutes"], measured["n"], measured["source"]
    if n and eta:
        value, label = scale
        return math.ceil(eta * value if value else eta), n, f"{used} {label}"
    if fallback is None:
        return None, 0, "uncalibrated: no box chains and no project median"
    return math.ceil(fallback), 0, "uncalibrated: project median brief-to-landed"


def schedule(boxes, width, landing):
    """Minutes to finish every box: list scheduling at `width` parallel workers, longest remaining path first, each
    box taking its ETA plus the landing overhead and starting only after its dependencies land."""
    if width < 1:
        raise ValueError("width must be at least 1")
    duration = {k: b["eta"] + landing for k, b in boxes.items()}
    children = {k: [c for c, b in boxes.items() if k in b["deps"]] for k in boxes}
    tails, visiting = {}, set()

    def tail(key):
        if key not in tails:
            if key in visiting:
                raise ValueError(f"dependency cycle at {key}")
            visiting.add(key)
            tails[key] = duration[key] + max((tail(c) for c in children[key]), default=0)
        return tails[key]

    for key in boxes:
        tail(key)
    clock, done, running, pending = 0.0, {}, [], set(boxes)
    while pending or running:
        ready = sorted((k for k in pending if all(d in done for d in boxes[k]["deps"])), key=lambda k: (-tails[k], k))
        while ready and len(running) < width:
            key = ready.pop(0)
            pending.discard(key)
            running.append((clock + duration[key], key))
        running.sort()
        clock = running[0][0]
        while running and running[0][0] == clock:
            done[running.pop(0)[1]] = clock
    return clock


def serial(boxes, landing):
    return sum(b["eta"] + landing for b in boxes.values())


def estimate(project, issues, measure, metrics, width, now):
    """Critical path over the goal's boxes; a box chain already ends at its landing, so no landing overhead is added.
    `measure(facets)` gives box_durations for the boxes' facet texts."""
    boxes = parse_boxes(project, issues)
    measured = measure([box["facets"] for box in boxes.values()]) if boxes else {"boxes": [], "scale": None, "label": None}
    scale = measured["scale"], measured["label"] or "uncalibrated: no box row"
    landing = 0.0
    ns = []
    for box, one in zip(boxes.values(), measured["boxes"]):
        box["eta"], n, box["source"] = box_eta(one, scale, metrics.get("median_brief_to_landed_min"))
        ns.append(str(n) if n else "uncal")
    if any(b["eta"] is None for b in boxes.values()):
        return {"project": project, "boxes": len(boxes), "width": width, "minutes": None, "boxes_detail": boxes, "ns": ns}
    minutes = schedule(boxes, width, landing) if boxes else 0.0
    hand = serial(boxes, landing)
    return {"project": project, "boxes": len(boxes), "width": width, "landing": landing, "minutes": minutes,
            "finish": iso(now + timedelta(minutes=minutes)), "serial_minutes": hand,
            "serial_finish": iso(now + timedelta(minutes=hand)), "ns": ns, "boxes_detail": boxes}


def line(result, target):
    project = result["project"]
    if not result["boxes"]:
        return f"goal {project}: no open goal boxes"
    if result["minutes"] is None:
        return f"goal {project}: ETA n/a over {result['boxes']} boxes (a box has no category cell and no project median)"
    verdict = ""
    if target is not None:
        slack = (stamp(target) - stamp(result["finish"])).total_seconds() / 60
        verdict = f" vs target {iso(stamp(target))} ({span(abs(slack))} {'early' if slack >= 0 else 'late'})"
    return (f"goal {project}: ETA {span(result['minutes'])} critical path over {result['boxes']} boxes at width "
            f"{result['width']} (box = first staffed to last landed), finish {result['finish']}{verdict}; "
            f"serial {span(result['serial_minutes'])}; n per box {','.join(result['ns']) or '-'}")


def gate_width(project, concurrency):
    """(width, note): the measured concurrency while the spawn gate admits this project's spawns (CPU and pressure
    below the capacity gate's limits and at least one account without a usage refusal), else 1 (queued behind a lease)."""
    hooks = LIB.parent / "hooks/lib"
    if str(hooks) not in sys.path:
        sys.path.insert(0, str(hooks))
    import spawn_capacity
    reading = spawn_capacity.capacity()
    cpu = reading is None or (reading[0] < reading[1] and reading[2] <= spawn_capacity.PROTECTED_PRESSURE_LIMIT)
    accounts = []
    verdict = spawn_capacity.usage_verdict({"tool_name": "Agent"})
    for account in verdict.get("paces") or {}:
        accounts.append((account, verdict.get("refusal") is None))
    open_accounts = [a for a, ok in accounts if ok]
    width = max(1, concurrency) if cpu and (open_accounts or not accounts) else 1
    return width, f"gate: cpu {'open' if cpu else 'full'}, accounts open {','.join(open_accounts) or 'none'}"


def landing_minutes(project):
    try:
        shown = subprocess.run(["north", "flow", "row", project, "push_to_land_minutes"], capture_output=True, text=True, timeout=120, stdin=subprocess.DEVNULL, check=True).stdout
        value = json.loads(shown)["row"]["push_to_land_minutes"]
        return float(value) if isinstance(value, (int, float)) and not isinstance(value, bool) else 0.0
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError):
        return 0.0


def goal_issues(project, config):
    fields = "number,body,state,closedAt,labels,blockedBy"
    numbers = [int(str(n).split("#")[-1]) for n in config.get("goal", [])]
    if numbers:
        return [json.loads(subprocess.run(["gh", "issue", "view", str(n), "--repo", f"tompassarelli/{project}", "--json", fields],
                                          capture_output=True, text=True, timeout=30, check=True).stdout) for n in numbers]
    return json.loads(subprocess.run(["gh", "issue", "list", "--repo", f"tompassarelli/{project}", "--state", "open",
                                      "--label", "priority:now", "--limit", "100", "--json", fields],
                                     capture_output=True, text=True, timeout=30, check=True).stdout)


def record(row):
    """Append one goal row to the NorthThreads calibration table through `threads calibrate --write`."""
    timeout = 5
    try:
        result = subprocess.run([os.environ.get("NORTH_BIN") or "north", "threads", "calibrate", "--write", "--goal-row", json.dumps(row, allow_nan=False)],
                                capture_output=True, text=True, timeout=timeout)
    except subprocess.TimeoutExpired:
        print(f"warning: threads calibrate --write timed out after {timeout}s; goal ETA row not recorded", file=sys.stderr)
        return None
    return None if result.returncode == 0 else (result.stderr.strip() or f"exit {result.returncode}")


def snapshot(db_path):
    """All runs from a read-only ledger."""
    db = open_ledger(db_path)
    db.row_factory = sqlite3.Row
    return [dict(r) for r in db.execute("SELECT * FROM runs")]


def plan_lines(cfg, db_path, now=None, write=True):
    now = now or datetime.now(timezone.utc)
    lines = []
    for project, config in sorted(cfg.get("projects", {}).items()):
        if not isinstance(config, dict) or "target" not in config:
            continue
        try:
            rows = snapshot(db_path)
            metrics = ledger_context.delivery_metrics(rows, project)
            width, note = gate_width(project, ledger_context.delivery_metrics(rows)["concurrency"])
            result = estimate(project, goal_issues(project, config), lambda facets: box_durations(facets, db_path=db_path), metrics, width, now)
        except (OSError, ValueError, KeyError, TypeError, sqlite3.Error, subprocess.SubprocessError) as error:
            lines.append(f"goal {project}: ETA n/a ({error})")
            continue
        lines.append(line(result, config["target"]) + f"; {note}")
        if write and result["minutes"] is not None and result["boxes"]:
            failed = record({"kind": "goal_eta", "cell": project, "n": result["boxes"], "value": result["minutes"],
                             "source": json.dumps({"predicted_finish": result["finish"], "boxes": result["boxes"],
                                                   "width": result["width"], "target": iso(stamp(config["target"]))})})
            if failed:
                lines.append(f"goal {project}: prediction not recorded ({failed})")
    return lines


def actual_row(project, predictions, previous_score, issues):
    """The goal_actual row for a finished goal: its first goal_eta prediction since the previous score against the
    last issue close, or None while any issue is open or nothing was predicted."""
    if not issues or any(i.get("state", "").upper() != "CLOSED" or not i.get("closedAt") for i in issues):
        return None
    since = stamp(previous_score) if previous_score else None
    ours = sorted((p for p in predictions if p["kind"] == "goal_eta" and p["cell"] == project
                   and (since is None or stamp(p["at"]) > since)), key=lambda p: p["at"])
    if not ours:
        return None
    first = ours[0]
    detail = json.loads(first["source"])
    actual = max(stamp(i["closedAt"]) for i in issues)
    minutes = (actual - stamp(first["at"])).total_seconds() / 60
    return {"kind": "goal_actual", "cell": project, "n": first["n"], "value": minutes / first["value"] if first["value"] else None,
            "source": json.dumps({"predicted_at": first["at"], "predicted_finish": detail["predicted_finish"],
                                  "actual_finish": iso(actual), "actual_minutes": round(minutes, 1)})}


def replay(goal, ledger):
    """A recorded goal through the estimator as it stood at the goal's start: its boxes, the box chains of ledger runs
    ended before then with landings after it dropped, the recorded width, and no box calibration (none existed then)."""
    start = stamp(goal["start"])
    rows = []
    for row in ledger:
        if stamp(row["ended"]) < start:
            late = row["landed_at"] and stamp(row["landed_at"]) >= start
            rows.append({**row, **({"landed": 0, "landed_at": None} if late else {})})
    return estimate("north", goal["issues"], lambda facets: box_durations(facets, rows=rows), goal["metrics"], goal["width"], start)
