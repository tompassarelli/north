import json
import os
from pathlib import Path
import re
import sqlite3
import subprocess
import sys
import tomllib
from datetime import datetime, timedelta, timezone

sys.path.insert(0, str(Path(__file__).resolve().parent))
from calibration import open_ledger


def stamp(value):
    result = value if isinstance(value, datetime) else datetime.fromisoformat(str(value).replace("Z", "+00:00"))
    return result if result.tzinfo else result.replace(tzinfo=timezone.utc)


def costs_path():
    return Path(os.environ.get("AGENTS_OPERATOR_DIR") or Path.home() / "code/south/main/agents") / "costs.toml"


def missed_target(project, target, costs=None):
    if costs is None:
        try:
            costs = tomllib.loads(costs_path().read_text())["service"]
        except (OSError, ValueError, KeyError):
            costs = []
    levers = []
    for service in costs:
        if service.get("kind") == "ai-plan":
            continue
        action = "rent build/test compute" if service.get("kind") in ("cloud", "gpu-rental") and service.get("billing") != "fixed" else "reconfigure/swap service"
        amount = service.get("usd_per_week")
        price = f"current ledger cost ${amount:g}/week" if isinstance(amount, (float, int)) else "$ unmeasured"
        levers.append(f"{action}: {service['id']} {price}, speedup unmeasured")
    if not levers:
        levers = ["rent build/test compute / reconfigure/swap service: $ unmeasured, speedup unmeasured"]
    return (f"Blocked: decision for Tom: {project} projected miss {target}: "
            f"more budget ({'; '.join(levers)}) / later date / smaller scope")


def score(project, target, now, issues, runs, budget):
    target = stamp(target)
    if now < target:
        raise ValueError("project target has not passed")
    complete = bool(issues) and all(i.get("state", "").upper() == "CLOSED" and i.get("closedAt") for i in issues)
    on_date = "unmeasured" if issues is None else ("pass" if complete and all(stamp(i["closedAt"]) <= target for i in issues) else "fail")
    boxes = None if issues is None else [box for issue in issues for box in re.findall(r"^\s*[-*] \[([ xX])\]", issue.get("body", ""), re.M)]
    spec = "unmeasured" if issues is None else ("pass" if all(box.lower() == "x" for box in boxes) else "fail")
    spent = None if not runs or any(r.get("cost_usd") is None for r in runs) else sum(r["cost_usd"] for r in runs)
    in_budget = "unmeasured" if spent is None or budget is None else ("pass" if spent <= budget else "fail")
    return {"project": project, "target": target.isoformat(), "scored_at": now.isoformat(),
            "on-date": on_date, "in-budget": in_budget, "spec-pass": spec, "spent_usd": spent, "budget_usd": budget}


def main(argv):
    if len(argv) != 3:
        raise ValueError("usage: agents plan --score PROJECT")
    project, config_path = argv[1:]
    cfg = tomllib.loads(Path(config_path).read_text())
    config = cfg["projects"][project]
    target = stamp(config["target"])
    now = datetime.now(timezone.utc)
    if now < target:
        raise ValueError("project target has not passed")
    from project_plan import snapshot_runs
    try:
        all_runs = snapshot_runs()
    except (sqlite3.Error, OSError):
        all_runs = []
    since = target - timedelta(days=7)
    runs = [r for r in all_runs if since <= stamp(r["ended"]) <= target and
            re.search(rf"(?:^|\s){re.escape(project)}#\d+\b", r.get("item") or "")]
    import calibration
    import goal_eta
    try:
        issues = goal_eta.goal_issues(project, config) or None
    except (subprocess.SubprocessError, OSError, ValueError):
        issues = None
    budget = None
    db_path = Path(os.environ.get("THREADS_DB") or Path(os.environ.get("XDG_STATE_HOME") or Path.home() / ".local/state") / "threads/snapshot.db").resolve()
    try:
        import plan_usage
        import usage
        import routing
        costs = tomllib.loads(costs_path().read_text())
        with open_ledger(db_path) as db:
            rows = usage.latest(db)
            ctx = plan_usage.live_ctx(cfg, {}, rows, db, now, cfg.get("tiers", {}), costs) if rows else {"accounts": {}}
        accounts = list(ctx["accounts"].values())
        if accounts and "share" in config and all(not routing.stale(a) for a in accounts):
            budget = sum(routing.account_metrics(a, now)["available_usd"] for a in accounts) * config["share"] / 100
    except (OSError, ValueError, sqlite3.Error):
        pass
    row = score(project, target, now, issues, runs, budget)
    state = Path(os.environ.get("XDG_STATE_HOME") or Path.home() / ".local/state") / "agents/goal-scores.jsonl"
    state.parent.mkdir(parents=True, exist_ok=True)
    try:
        previous = [json.loads(l) for l in state.read_text().splitlines() if l.strip()]
    except OSError:
        previous = []
    actual = goal_eta.actual_row(project, calibration.rows(db_path), next(
        (p["scored_at"] for p in reversed(previous) if p.get("project") == project), None), issues)
    failed = goal_eta.record(actual) if actual else None
    if failed:
        print(f"{project} goal ETA actual not recorded: {failed}", file=sys.stderr)
    with state.open("a") as stream:
        stream.write(json.dumps(row, allow_nan=False) + "\n")
    for verdict in ("on-date", "in-budget", "spec-pass"):
        print(f"{project} {verdict}: {row[verdict]}")


if __name__ == "__main__":
    try:
        main(sys.argv)
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        sys.exit(f"agents plan --score: {error}")
