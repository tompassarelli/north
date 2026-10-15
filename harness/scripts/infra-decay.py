#!/usr/bin/env python3
import argparse
from collections import Counter, defaultdict
from datetime import datetime, timedelta, timezone
import json
import os
from pathlib import Path
import re
from statistics import median
import subprocess
import sys


def instant(value):
    return datetime.fromisoformat(value.replace("Z", "+00:00")).astimezone(timezone.utc)


def command(args):
    return subprocess.run(args, check=True, capture_output=True, text=True, timeout=45).stdout


def read_json(path, default):
    return json.loads(path.read_text()) if path.exists() else default


def collect(now, home=None):
    home = home or Path.home()
    state = Path(os.environ.get("XDG_STATE_HOME", home / ".local/state")) / "agents"
    code = home / "code"
    ledger = read_json(state / "iteration-ledger/changes.json", {})
    health = read_json(state / "test-health.json", {})
    org = read_json(state / "org.json", {})
    owners = {}
    for node in org.get("nodes", []):
        domain = node.get("domain", "")
        if (node.get("role") == "lead" and node.get("state") == "active"
                and node["id"] in (f"{domain}-lead", f"{domain}-claude-lead", f"{domain}-codex-lead")):
            owners.setdefault(domain, node["id"])
    snapshot = dict(changes=ledger.get("changes", []), workflows=[], commits={},
                    files=[], timers=[], owners=owners, routines={})
    routines = code / "south/main/agents/routines"
    if routines.exists():
        for path in sorted(routines.glob("*.md")):
            text = path.read_text()
            name = re.search(r"^name: (.+)$", text, re.M)
            owner = re.search(r"^owner: (.+)$", text, re.M)
            if name and owner:
                snapshot["routines"][name[1].split("@")[0]] = owner[1]
    for name, period, owner in (
            ("test-health.json", 86400, "test-audit-lead"),
            ("landing-health.json", 120, "north-codex-lead"),
            ("iteration-ledger/changes.json", 3600, "throughput-architect-lead"),
            ("worker-ledger.sqlite", 900, "north-codex-lead")):
        path = state / name
        modified = path.stat().st_mtime if path.exists() else None
        if name == "worker-ledger.sqlite":
            wal = Path(str(path) + "-wal")
            if wal.exists():
                modified = max(modified or 0, wal.stat().st_mtime)
        snapshot["files"].append(dict(name=name, modified=modified, period_s=period, owner=owner))
    timers = json.loads(command(["systemctl", "--user", "list-timers", "--all", "--no-pager", "--output=json"]))
    enabled = json.loads(command(["systemctl", "--user", "list-unit-files", "--type=timer", "--no-pager", "--output=json"]))
    enabled_names = {row["unit_file"] for row in enabled if row["state"] in ("enabled", "enabled-runtime")}
    by_name = {row["unit"]: row for row in timers}
    snapshot["timers"] = [dict(name=name, enabled=True, next=by_name.get(name, {}).get("next"))
                          for name in sorted(enabled_names)]
    budgets = {row["repo"]: row.get("ci", []) for row in health.get("repos", [])}
    repos = sorted(set(ledger.get("projects", {})) | set(budgets))
    for repo in repos:
        checkout = code / ("nixos-config" if repo == "firn" else repo) / "main"
        if not checkout.exists():
            continue
        remote_read = subprocess.run(["git", "-C", str(checkout), "remote", "get-url", "origin"],
                                     capture_output=True, text=True, timeout=10)
        if remote_read.returncode:
            continue
        remote = remote_read.stdout.strip()
        match = re.search(r"github\.com[:/](.+?)(?:\.git)?$", remote)
        if not match:
            continue
        slug = match[1]
        runs = json.loads(command(["gh", "run", "list", "-R", slug, "--limit", "1000", "--created",
                                   ">=" + (now - timedelta(days=8)).isoformat(), "--json",
                                   "databaseId,workflowName,createdAt,startedAt,updatedAt,status,conclusion"]))
        groups = defaultdict(list)
        for run in runs:
            groups[run["workflowName"]].append(run)
        routing = read_json(checkout / "ts/runner-routing.json", {})
        labels = {label for value in routing.values() for label in (value if isinstance(value, list) else [value])
                  if label not in ("ubuntu-latest", "windows-latest", "macos-latest")}
        runners = []
        if labels:
            runners = json.loads(command(["gh", "api", f"repos/{slug}/actions/runners?per_page=100"]))["runners"]
        for label in sorted(labels):
            snapshot["workflows"].append(dict(repo=repo, name=f"runner:{label}", label=label,
                online=sum(r["status"] == "online" and label in [v["name"] for v in r["labels"]] for r in runners)))
        for name, rows in sorted(groups.items()):
            workflow = dict(repo=repo, name=name, runs=rows, budget_s=None)
            for gate in budgets.get(repo, []):
                if gate["gate"].lower().removesuffix(".yml") == name.lower().replace(" ", "-"):
                    workflow["budget_s"] = gate.get("budget_s")
            snapshot["workflows"].append(workflow)
        output = command(["git", "-C", str(checkout), "log", "origin/main", "--no-merges",
                          f"--since={(now - timedelta(days=1)).isoformat()}", f"--until={now.isoformat()}",
                          "--format=%H%x09%s"])
        snapshot["commits"][repo] = [dict(sha=line.split("\t", 1)[0], subject=line.split("\t", 1)[1])
                                      for line in output.splitlines() if "\t" in line]
    return snapshot


def evaluate(snapshot, now):
    rows = []
    day = now.replace(hour=0, minute=0, second=0, microsecond=0)

    def add(check, repo, value, baseline, threshold, firing, owner, detail):
        rows.append(dict(check=check, repo=repo, value=value, baseline=baseline, threshold=threshold,
                         firing=bool(firing), owner=owner,
                         brief=f"Item: {repo} {check}\nCategory: tooling\nSpec: measured\nScope: module\n"
                               f"Surface: shell\nVerify: local-test\nNeeds-tom: no\n"
                               f"Payback: restore the decayed {check} signal\nETA: 25 min (uncalibrated)\n"
                               f"Run while Tom is away/asleep. {detail}\n"
                               f"Done when: rerun infra-decay.py --json and this check is below its threshold."))

    changes = defaultdict(list)
    for change in snapshot.get("changes", []):
        if change.get("landed_at") and instant(change["landed_at"]) <= now:
            changes[change["repo"]].append(change)
    for repo, values in sorted(changes.items()):
        asks = [c for c in values if c.get("tom_ask") and c.get("total_minutes") is not None]
        today = [c for c in asks if instant(c["landed_at"]) >= day]
        past = [c["total_minutes"] for c in asks if day - timedelta(days=7) <= instant(c["landed_at"]) < day]
        current = median([c["total_minutes"] for c in today]) if today else None
        baseline = median(past) if past else None
        bottleneck = Counter(c.get("bottleneck_stage") for c in today).most_common(1)
        add("velocity", repo, dict(median_minutes=current, n=len(today)), baseline,
            dict(ratio=1.5, minimum_n=3), len(today) >= 3 and baseline is not None and current > 1.5 * baseline,
            "throughput-architect-lead", f"Top bottleneck stage: {bottleneck[0][0] if bottleneck else 'unknown'}.")
    for workflow in snapshot.get("workflows", []):
        repo, name = workflow["repo"], workflow["name"]
        owner = snapshot.get("owners", {}).get(repo, "test-audit-lead")
        if "label" in workflow:
            add("runner-label", repo, dict(label=workflow["label"], online=workflow["online"]), 1, 1,
                workflow["online"] == 0, owner, f"Restore an online runner for {workflow['label']}.")
            continue
        runs = [r for r in workflow.get("runs", []) if instant(r["createdAt"]) <= now]
        recent = [r for r in runs if instant(r["createdAt"]) >= now - timedelta(hours=48)]
        successes = sum(r.get("conclusion") == "success" for r in recent)
        cancelled = sum(r.get("conclusion") == "cancelled" for r in recent)
        queued = [max(0, (now - instant(r["createdAt"])).total_seconds() / 60) for r in recent
                  if r.get("status") in ("queued", "pending", "waiting", "requested")]
        oldest = max(queued, default=0)
        add("workflow-jobs", repo, dict(workflow=name, runs=len(recent), successes=successes,
            cancelled=cancelled, oldest_queue_minutes=round(oldest, 1)), None,
            dict(minimum_runs=5, minimum_successes=1, queue_minutes=120),
            (len(recent) >= 5 and successes == 0) or oldest > 120, owner,
            f"Restore {name}: {successes}/{len(recent)} succeeded, {cancelled} cancelled; oldest queue {oldest:.1f} min.")
        if workflow.get("budget_s") is None and not re.search(r"autoland|landing[ -]gate|farm[ -]test", name, re.I):
            continue
        walls = []
        for run in runs:
            if run.get("conclusion") == "success" and run.get("startedAt") and run.get("updatedAt"):
                ended = instant(run["updatedAt"])
                wall = (ended - instant(run["startedAt"])).total_seconds()
                if ended <= now and wall >= 0:
                    walls.append((ended, wall))
        today = [wall for at, wall in walls if at >= day]
        past = [wall for at, wall in walls if day - timedelta(days=7) <= at < day]
        if today:
            current, baseline = median(today), median(past) if past else None
            budget = workflow.get("budget_s")
            add("gate-suite-time", repo, dict(workflow=name, p50_s=current, n=len(today)), baseline,
                dict(budget_s=budget, ratio=1.25),
                (budget is not None and current > budget) or (baseline is not None and current > 1.25 * baseline),
                "suite-speed-lead", f"Reduce {name} wall time, now {current:.1f}s (run startedAt to updatedAt).")
    for file in snapshot.get("files", []):
        age = max(0, now.timestamp() - file["modified"]) if file.get("modified") is not None else None
        name = file["name"]
        routine = name.rsplit("/", 1)[-1].split(".")[0]
        owner = snapshot.get("routines", {}).get(routine, file["owner"])
        add("stale-file", name, dict(age_s=round(age, 1) if age is not None else None), file["period_s"],
            2 * file["period_s"], age is None or age > 2 * file["period_s"], owner,
            f"Restore the writer of {name}; expected period {file['period_s']}s.")
    for timer in snapshot.get("timers", []):
        owner = snapshot.get("routines", {}).get(timer["name"].removesuffix(".timer"), "north-codex-lead")
        add("dead-timer", timer["name"], dict(enabled=timer["enabled"], next=timer.get("next")),
            "scheduled", "enabled timer has a next run", timer["enabled"] and not timer.get("next"),
            owner, f"Restore {timer['name']} and verify its next run is scheduled.")
    for repo, commits in sorted(snapshot.get("commits", {}).items()):
        records = sum(bool(re.search(r"\brecords?\b|\bregenerat|\bre-?record", c["subject"], re.I)) for c in commits)
        share = records / len(commits) if commits else 0
        add("record-churn", repo, dict(record_commits=records, commits=len(commits), share=share), None, 0.1,
            share > 0.1, "test-audit-lead", "Remove forced record refreshes using the testing skill churn guidance; "
            "record subjects use iteration-ledger's record/regenerate heuristic.")
    return rows


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--json", action="store_true")
    parser.add_argument("--at", type=instant)
    args = parser.parse_args()
    now = args.at or datetime.now(timezone.utc)
    try:
        rows = evaluate(collect(now), now)
    except subprocess.CalledProcessError as error:
        print(f"infra-decay: source command failed ({error.returncode}): {' '.join(error.cmd)}", file=sys.stderr)
        return 1
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        print(f"infra-decay: could not read sources ({type(error).__name__})", file=sys.stderr)
        return 1
    if args.json:
        print(json.dumps(rows, indent=2))
    else:
        for row in rows:
            print(f"{'FIRE' if row['firing'] else 'ok'}\t{row['check']}\t{row['repo']}\t{json.dumps(row['value'])}\t{row['owner']}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
