import argparse
import fcntl
from datetime import datetime, timezone
import json
import math
import os
import re
import statistics
import subprocess
import sqlite3
import sys
import time
import tomllib
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from calibration import open_ledger
from defaults import DEFAULTS
from project_score import missed_target
from laptop_line import laptop_line

DEFAULT_REPOS = ("smashcraft", "wisp", "north", "smashcraft-client")
CACHE_SECONDS = 300
MEDIAN_SECONDS = 6 * 3600
ACCOUNT_JOBS = 40
IDLE_JOBS = 10
IDLE_PENDING = 5
IDLE_SECONDS = 900
WISP_REPO = "smashcraft"
NATIVE_LABELS = ("needs:native-pair", "needs:native-single")
CENSUS_REUSE_FIELD = "census-reused"
REUSE_WINDOW = 20
CARGO_MANIFESTS = ("Cargo.toml", "src-tauri/Cargo.toml")
CARGO_PIN = re.compile(r'git\s*=\s*"https://github\.com/([\w.-]+/[\w.-]+?)(?:\.git)?"[^}\n]*?\btag\s*=\s*"([^"]+)"')
WISP_STATUS = ["bun", "wisp", "lan", "status"]
WISP_TS = Path.home() / "code/smashcraft/main/ts"
CALLS = []
JOBS = {}
CLAIMS = {}
THROUGHPUT_WINDOW = 3 * 3600
CLOSE_FLAG_MINUTES = 30
UNOWNED_SECONDS = 900
AUTO_CLOSE = re.compile(r"Closed by auto-close after ([0-9a-f]{7,64}) landed")
LAND_REF = re.compile(r"\b(?:fix(?:e[sd])?|close[sd]?|refs?)\b:?\s+((?:[\w.-]+/)?[\w.-]+)?#(\d+)(?!\d)", re.I)


def state_dir():
    return Path(os.environ.get("XDG_STATE_HOME") or Path.home() / ".local/state")


def github_repo(url):
    match = re.fullmatch(r"(?:https://github\.com/|git@github\.com:|ssh://git@github\.com/)([\w.-]+/[\w.-]+?)(?:\.git)?/?", url.strip())
    return match.group(1) if match else None


def run(argv, cwd=None, input=None):
    if argv[0] == "gh":
        CALLS.append(argv[1])
    try:
        result = subprocess.run(argv, cwd=cwd, input=input, capture_output=True, text=True,
                                timeout=20, stdin=subprocess.DEVNULL if input is None else None)
        return result.stdout if result.returncode == 0 else None
    except (OSError, subprocess.TimeoutExpired):
        return None


def cache_path(repo):
    return state_dir() / "agents/flow" / f"{repo}.json"


def valid_name(repo):
    return re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]*", repo) is not None


def cached(repo, now, fields=("reuse",)):
    try:
        row = json.loads(cache_path(repo).read_text())
        if row["repo"] == repo and all(field in row for field in fields) and 0 <= now - row["epoch"] < CACHE_SECONDS:
            return row
    except (OSError, ValueError, KeyError, TypeError):
        pass
    return None


def patch_id(path, sha):
    patch = run(["git", "show", "--pretty=format:", "--no-ext-diff", sha], path)
    out = run(["git", "patch-id", "--stable"], path, patch) if patch else None
    return out.split()[0] if out and out.split() else None


def latency(path, slug, commits):
    try:
        lines = (state_dir() / "safe-push/pushes.tsv").read_text().splitlines()
    except FileNotFoundError:
        return None, None
    except OSError:
        return "error", "error"
    pushes = {}
    for line in lines:
        fields = line.split("\t")
        if len(fields) < 4 or fields[1] != slug.split("/")[-1]:
            continue
        try:
            at = float(fields[0])
        except ValueError:
            continue
        sha = fields[3]
        if re.fullmatch(r"[0-9a-f]{40,64}", sha):
            pushes[sha] = min(at, pushes.get(sha, at))
    patches = None
    times = dict(commits)
    samples = []
    for sha, pushed in pushes.items():
        landed = times.get(sha)
        if landed is None:
            pid = patch_id(path, sha)
            if pid is None:
                continue
            if patches is None:
                patches = {patch_id(path, head): at for head, at in reversed(commits)}
            landed = patches.get(pid)
        if landed is not None:
            samples.append(max(0, landed - pushed) / 60)
    return percentiles(samples)


def percentiles(samples):
    if not samples:
        return None, None
    return statistics.median(samples), sorted(samples)[math.ceil(len(samples) * 0.9) - 1]


def timestamp(value):
    return datetime.fromisoformat(value.replace("Z", "+00:00")).timestamp()


def autoland_samples(pushes, tips, now):
    pushed = {}
    for push in pushes:
        at = timestamp(push["createdAt"])
        if push["headBranch"].startswith("claude/land-") and now - 86400 <= at <= now:
            sha = push["headSha"]
            pushed[sha] = min(at, pushed.get(sha, at))
    samples, reuse = [], []
    for tip in tips.values():
        if not tip or tip["oid"] not in pushed or not tip["status"]:
            continue
        for status in tip["status"]["contexts"]:
            if status["context"] != "autoland" or status["state"] != "SUCCESS":
                continue
            landed_sha = re.match(r"landed as ([0-9a-f]{40,64})(?:\s|$)", status["description"] or "")
            if not landed_sha:
                continue
            landed = timestamp(status["createdAt"])
            if pushed[tip["oid"]] <= landed <= now:
                samples.append((landed - pushed[tip["oid"]]) / 60)
                field = re.search(rf"\b{re.escape(CENSUS_REUSE_FIELD)}[=:] ?(yes|no|true|false)\b",
                                  status["description"] or "")
                reuse.append((landed, field and field.group(1) in ("yes", "true")))
            break
    return samples, [flag for _, flag in sorted(reuse, key=lambda item: item[0], reverse=True)]


def landed_times(tips):
    times = []
    for tip in tips.values():
        for status in ((tip or {}).get("status") or {}).get("contexts") or []:
            if (status["context"] == "autoland" and status["state"] == "SUCCESS"
                    and re.match(r"landed as [0-9a-f]{40,64}(?:\s|$)", status["description"] or "")):
                times.append(timestamp(status["createdAt"]))
                break
    return times


def reuse_verdict(flags):
    if flags is None:
        return {"text": "reuse n/a", "reason": None}
    recent = flags[:REUSE_WINDOW]
    if not recent or any(flag is None for flag in recent):
        return {"text": "reuse n/a", "reason": None}
    reused = sum(recent)
    text = f"reuse {reused}/{len(recent)}"
    return {"text": text, "reason": f"census {text} below 50%" if reused * 2 < len(recent) else None}


def cargo_pins(manifests):
    return sorted({match.groups() for text in manifests for match in CARGO_PIN.finditer(text)})


def pins_verdict(builds):
    if builds is None:
        return {"text": "pins n/a", "reason": None}
    red = [f"{dep}@{tag} built {conclusion}" for dep, tag, status, conclusion in builds
           if status == "completed" and conclusion != "success"]
    return {"text": f"pins {len(builds) - len(red)}/{len(builds)} green",
            "reason": "pinned tag build not green: " + ", ".join(red) if red else None}


def pin_builds(path):
    manifests = [text for name in CARGO_MANIFESTS
                 if (text := run(["git", "show", f"origin/main:{name}"], path)) is not None]
    builds = []
    for dep, tag in cargo_pins(manifests):
        raw = run(["gh", "run", "list", "--repo", dep, "--branch", tag, "--limit", "1", "--json", "status,conclusion"])
        try:
            last = json.loads(raw)[0]
        except (ValueError, TypeError, IndexError):
            return None
        builds.append((dep, tag, last["status"], last["conclusion"]))
    return builds


def actions_verdict(pending, samples, now):
    window = [busy for at, busy in samples if now - IDLE_SECONDS - CACHE_SECONDS <= at <= now]
    covered = any(now - IDLE_SECONDS - CACHE_SECONDS <= at <= now - IDLE_SECONDS for at, _ in samples)
    current = max((at, busy) for at, busy in samples)[1] if samples else None
    text = f"actions {current if current is not None else '?'}/{ACCOUNT_JOBS} busy"
    flagged = (isinstance(pending, int) and pending >= IDLE_PENDING and covered
               and max(window) < IDLE_JOBS)
    return {"text": text, "reason": (f"idle Actions slots: at most {max(window)}/{ACCOUNT_JOBS} jobs busy "
                                     f"for {IDLE_SECONDS // 60}m with {pending} pending") if flagged else None}


def epoch(value):
    if isinstance(value, (int, float)):
        return value / 1000 if value > 1e12 else value
    return timestamp(value)


def wisp_verdict(pairs, waiting, now):
    if pairs is None:
        return {"text": "wisp no pool", "reason": None}
    idle = []
    dead = [(pair.get("pair"), pair.get("down") or []) for pair in pairs if pair.get("state") == "dead"]
    for pair in pairs:
        driver = pair.get("driver") or {}
        if "state" in pair:
            seconds = pair.get("idleSeconds", 0) if pair["state"] == "idle" else 0
        elif driver.get("goneAt") is not None:
            seconds = max(0, now - epoch(driver["goneAt"]))
        else:
            continue
        if seconds > IDLE_SECONDS:
            idle.append((pair.get("pair"), seconds))
    text = (f"wisp {len(idle)}/{len(pairs)} pairs idle >{IDLE_SECONDS // 60}m"
            + (f", {len(dead)} dead" if dead else "") + f", native-check waiting {waiting}")
    flagged = (idle or dead) and isinstance(waiting, int) and waiting >= 1
    parts = ([f"pair {name} idle {seconds / 60:.0f}m" for name, seconds in idle]
             + [f"pair {name} dead: {' '.join(down) or 'a client'} down" for name, down in dead])
    return {"text": text, "reason": ("idle Wisp pool: " + ", ".join(parts)
                                     + f" with {waiting} native-check waiting") if flagged else None}


def wisp_pool():
    try:
        result = subprocess.run(WISP_STATUS, cwd=WISP_TS, capture_output=True, text=True, timeout=40, stdin=subprocess.DEVNULL)
    except (OSError, subprocess.TimeoutExpired):
        return "error"
    if result.returncode != 0:
        return None if "no pool is running" in result.stdout + result.stderr else "error"
    pairs = []
    for line in result.stdout.splitlines():
        try:
            item = json.loads(line)
        except ValueError:
            continue
        if isinstance(item, dict) and "pair" in item:
            pairs.append(item)
    return pairs


def native_waiting(slug):
    raw = run(["gh", "issue", "list", "--repo", slug, "--state", "open", "--limit", "100",
               "--search", "label:" + ",".join(NATIVE_LABELS), "--json", "number"])
    try:
        return len(json.loads(raw))
    except (ValueError, TypeError):
        return "error"


def wisp_row(slug, now):
    pairs = wisp_pool()
    if pairs == "error":
        return {"text": "wisp error", "reason": None}
    probe = wisp_verdict(pairs, 1, now)
    return wisp_verdict(pairs, native_waiting(slug) if probe["reason"] else 0, now) if pairs else probe


def autoland_latency(slug, now):
    since = datetime.fromtimestamp(now - 86400, timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    raw = run(["gh", "run", "list", "--repo", slug, "--workflow", "autoland.yml",
               "--event", "push", "--created", ">=" + since, "--limit", "1000",
               "--json", "headBranch,headSha,createdAt"])
    try:
        pushes = json.loads(raw)
        shas = list(dict.fromkeys(push["headSha"] for push in pushes
                                 if push["headBranch"].startswith("claude/land-")))
        if not shas:
            return None, None, [], [], []
        owner, name = slug.split("/")
        objects = " ".join(f'tip{i}: object(oid:{json.dumps(sha)}) {{ ... on Commit {{ '
                           'oid status { contexts { context state description createdAt } } } }'
                           for i, sha in enumerate(shas))
        query = (f'{{ repository(owner:{json.dumps(owner)}, name:{json.dumps(name)}) {{ '
                 + objects + " } }")
        raw = run(["gh", "api", "graphql", "--input", "-"], input=json.dumps({"query": query}))
        response = json.loads(raw)
        if response.get("errors"):
            return "error", "error", None, land_push_times(pushes), None
        samples, reuse = autoland_samples(pushes, response["data"]["repository"], now)
        return (*percentiles(samples), reuse, land_push_times(pushes), landed_times(response["data"]["repository"]))
    except (ValueError, TypeError, KeyError, AttributeError):
        return "error", "error", None, None, None


def land_push_times(pushes):
    first = {}
    for push in pushes:
        if push["headBranch"].startswith("claude/land-"):
            at = timestamp(push["createdAt"])
            first[push["headSha"]] = min(at, first.get(push["headSha"], at))
    return list(first.values())


def local_push_times(slug):
    try:
        lines = (state_dir() / "safe-push/pushes.tsv").read_text().splitlines()
    except OSError:
        return []
    first = {}
    for fields in (line.split("\t") for line in lines):
        if len(fields) >= 4 and fields[1] == slug.split("/")[-1]:
            try:
                at = float(fields[0])
            except ValueError:
                continue
            first[fields[3]] = min(at, first.get(fields[3], at))
    return list(first.values())


def issue_refs(text, repo):
    return {int(number) for qualifier, number in LAND_REF.findall(text)
            if not qualifier or qualifier.split("/")[-1] == repo}


def project_targets():
    operator = Path(os.environ.get("AGENTS_OPERATOR_DIR") or Path.home() / "code/south/main/agents")
    path = Path(os.environ.get("AGENTS_ORCHESTRATION") or operator / "orchestration.toml")
    try:
        with path.open("rb") as source:
            projects = tomllib.load(source).get("projects", {})
    except FileNotFoundError:
        return {}
    targets = {}
    for name, project in projects.items():
        if "target" in project:
            target = datetime.fromisoformat(str(project["target"]).replace("Z", "+00:00"))
            targets[name] = (target if target.tzinfo else target.replace(tzinfo=timezone.utc)).timestamp()
    return targets


def throughput(repo, now, open_count, closed, main_log, landings, pushes, targets, dead_lanes, claims, pending_since):
    row = {"target": targets.get(repo) if targets else None}
    recent = [item for item in closed if now - 86400 <= timestamp(item["closedAt"]) <= now]
    row["closed_per_hour"] = (sum(timestamp(item["closedAt"]) >= now - THROUGHPUT_WINDOW for item in recent) / 3
                              if open_count is not None else None)
    hours = (targets[repo] - now) / 3600 if targets and repo in targets else 0
    row["needed_per_hour"] = (open_count / hours if hours > 0
                              and open_count is not None else None)
    referenced = {}
    for sha, at, message in main_log:
        for number in issue_refs(message, repo):
            referenced.setdefault(number, []).append(at)
    samples = []
    for item in recent:
        closed_at = timestamp(item["closedAt"])
        named = [match.group(1) for body in item.get("comments") or [] if (match := AUTO_CLOSE.search(body))]
        landed = next((at for sha, at, _ in main_log for prefix in named[-1:] if sha.startswith(prefix)), None)
        if landed is None:
            landed = max((at for at in referenced.get(item["number"], []) if at <= closed_at), default=None)
        if landed is not None:
            samples.append(max(0, closed_at - landed) / 60)
    row["land_to_close_minutes"] = statistics.median(samples) if samples else None
    pushed = sum(now - THROUGHPUT_WINDOW <= at <= now for at in pushes)
    row["land_ratio_3h"] = sum(now - THROUGHPUT_WINDOW <= at <= now for at in landings) / pushed if pushed else None
    row["failed_unowned"] = sum(now - failed_at > UNOWNED_SECONDS
                                and not any(claims.get(number, -math.inf) >= failed_at for number in issues)
                                for failed_at, issues in dead_lanes)
    row["oldest_lane_wait_minutes"] = max(0, now - min(pending_since)) / 60 if pending_since else None
    return row


def issue_activity(slug, now):
    owner, name = slug.split("/")
    since = datetime.fromtimestamp(now - 86400, timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    query = (f'{{ repository(owner:{json.dumps(owner)}, name:{json.dumps(name)}) {{ issues(states: OPEN) {{ totalCount }} }} '
             f'search(query:{json.dumps(f"repo:{slug} is:issue is:closed closed:>={since}")}, type: ISSUE, first: 100) '
             '{ nodes { ... on Issue { number closedAt comments(last: 20) { nodes { body } } } } } }')
    try:
        response = json.loads(run(["gh", "api", "graphql", "--input", "-"], input=json.dumps({"query": query})))
        data = response["data"]
        closed = [{"number": item["number"], "closedAt": item["closedAt"],
                   "comments": [comment["body"] for comment in item["comments"]["nodes"]]}
                  for item in data["search"]["nodes"] if item]
        return data["repository"]["issues"]["totalCount"], closed
    except (ValueError, TypeError, KeyError, AttributeError):
        return None, None


def claim_times(now):
    if "rows" not in CLAIMS:
        CLAIMS["rows"] = {}
        for line in (run(["threads", "list"]) or "").splitlines()[1:]:
            cols = line.split()
            item = re.fullmatch(r"([\w.-]+)#(\d+)", cols[0]) if len(cols) >= 3 else None
            clock = re.fullmatch(r"(\d+)m", cols[2]) if item else None
            if clock:
                CLAIMS["rows"].setdefault(item.group(1), {})[int(item.group(2))] = now - int(clock.group(1)) * 60
    return CLAIMS["rows"]


def autoland_states(slug, shas):
    if not shas:
        return {}
    owner, name = slug.split("/")
    objects = " ".join(f'tip{i}: object(oid:{json.dumps(sha)}) {{ ... on Commit {{ '
                       'oid status { contexts { context state createdAt } } } }'
                       for i, sha in enumerate(shas))
    query = f'{{ repository(owner:{json.dumps(owner)}, name:{json.dumps(name)}) {{ {objects} }} }}'
    try:
        response = json.loads(run(["gh", "api", "graphql", "--input", "-"], input=json.dumps({"query": query})))
        states = {}
        for tip in response["data"]["repository"].values():
            contexts = [c for c in ((tip or {}).get("status") or {}).get("contexts") or [] if c["context"] == "autoland"]
            if tip and contexts:
                states[tip["oid"]] = contexts[-1]
        return states
    except (ValueError, TypeError, KeyError, AttributeError):
        return {}


def lane_counts(shas, states, now):
    dead = [timestamp(states[sha]["createdAt"]) for sha in shas
            if sha in states and states[sha]["state"] in ("FAILURE", "ERROR")]
    oldest = max(0, now - min(dead)) / 60 if dead else None
    return len(shas) - len(dead), len(dead), oldest


def landing_run(item):
    if item["workflowName"] != "Autoland":
        return True
    title = item["displayTitle"]
    return (title == "Autoland train" or title.startswith("Autoland test ")
            or (title.startswith("Autoland ") and not title.startswith("Autoland plan")
                and not title.endswith("(push)")))


def workflow_median(runs, now):
    durations = [(timestamp(item["updatedAt"]) - timestamp(item["startedAt"])) / 60
                 for item in runs if landing_run(item) and item["conclusion"] == "success"
                 and now - 7 * 86400 <= timestamp(item["updatedAt"]) <= now]
    return statistics.median(durations) if durations else None


def stall_verdict(now, main_at, landings, pending, jobs, medians):
    times = sorted(at for at in landings if now - 86400 <= at <= now)
    gaps = [(b - a) / 60 for a, b in zip(times, times[1:])]
    gap = statistics.median(gaps) if gaps else None
    limit = max(30, 3 * gap) if gap is not None else 30
    age = max(0, now - main_at) / 60
    reasons = []
    overdue = []
    for job in jobs:
        median = medians.get(job["workflow"])
        minutes = max(0, now - job["started_at"]) / 60
        if median is not None and minutes > 2 * median:
            overdue.append({**job, "age_minutes": minutes, "limit_minutes": 2 * median})
    if pending > 0 and age > limit:
        reasons.append(f"main idle {age:.1f}m > {limit:.1f}m with {pending} pending")
    for job in overdue:
        reasons.append(f"{job['workflow']} run {job['run_id']} {job['name']} "
                       f"running {job['age_minutes']:.1f}m > {job['limit_minutes']:.1f}m "
                       f"on {job['runner']}")
    oldest = min(jobs, key=lambda job: job["started_at"], default=None)
    return {"stalled": bool(reasons), "reason": "; ".join(reasons) or "healthy",
            "main_age_minutes": age, "main_limit_minutes": limit, "median_gap_minutes": gap,
            "overdue_jobs": overdue, "runner": oldest["runner"] if oldest else None}


def run_jobs(slug, run_id):
    key = f"{slug}/{run_id}"
    if key not in JOBS:
        pages = json.loads(run(["gh", "api", "--paginate", "--slurp",
                                f"repos/{slug}/actions/runs/{run_id}/jobs?per_page=100"]))
        JOBS[key] = [job for page in pages for job in page["jobs"]]
    return JOBS[key]


def account_busy(now):
    directory = state_dir() / "agents/flow"
    path = directory / "actions-busy.json"
    try:
        saved = json.loads(path.read_text())
        samples, repos = saved["samples"], saved["repos"]
        if not 0 <= now - saved["repos_epoch"] < MEDIAN_SECONDS:
            repos = None
    except (OSError, ValueError, KeyError, TypeError):
        saved, samples, repos = {}, [], None
    if samples and 0 <= now - samples[-1][0] < CACHE_SECONDS:
        return samples
    try:
        if repos is None:
            since = datetime.fromtimestamp(now - 7 * 86400, timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
            listed = json.loads(run(["gh", "repo", "list", "--limit", "1000", "--json", "nameWithOwner,pushedAt"]))
            repos = [item["nameWithOwner"] for item in listed if (item["pushedAt"] or "") >= since]
            saved["repos_epoch"] = now
        busy = 0
        for slug in repos:
            active = json.loads(run(["gh", "run", "list", "--repo", slug, "--status", "in_progress",
                                     "--limit", "1000", "--json", "databaseId"]))
            for item in active:
                busy += sum(job["status"] == "in_progress" and bool(job.get("runner_name"))
                            for job in run_jobs(slug, item["databaseId"]))
    except (ValueError, TypeError, KeyError):
        return samples
    samples = [sample for sample in samples if now - sample[0] <= 3600] + [[now, busy]]
    directory.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(f".{os.getpid()}.tmp")
    temporary.write_text(json.dumps({**saved, "samples": samples, "repos": repos}) + "\n")
    temporary.replace(path)
    return samples


def landing_jobs(path, slug, now):
    config = run(["git", "config", "--blob", "origin/main:.safe-push", "--get-all", "ci.budget"], path)
    workflows = [line.split()[0] for line in (config or "").splitlines()]
    files = run(["git", "ls-tree", "--name-only", "origin/main", ".github/workflows/autoland.yml"], path)
    if files is None:
        raise ValueError("workflow discovery failed")
    if files.strip():
        workflows.append("autoland.yml")
    jobs, medians = [], {}
    median_path = state_dir() / "agents/flow" / f"{slug.replace('/', '--')}-medians.json"
    try:
        saved = json.loads(median_path.read_text())
        if not 0 <= now - saved["epoch"] < MEDIAN_SECONDS:
            saved = {"medians": {}}
    except (OSError, ValueError, KeyError, TypeError):
        saved = {"medians": {}}
    for workflow in dict.fromkeys(workflows):
        history = {}
        event = ["--event", "workflow_dispatch"] if workflow == "autoland.yml" else []
        for day in range(7 if workflow not in saved["medians"] else 0):
            bounds = [datetime.fromtimestamp(now - (day + offset) * 86400, timezone.utc)
                      .strftime("%Y-%m-%dT%H:%M:%SZ") for offset in (1, 0)]
            samples = json.loads(run(["gh", "run", "list", "--repo", slug, "--workflow", workflow,
                                      "--status", "success", "--created", "..".join(bounds), "--limit", "1000",
                                      "--json", "databaseId,workflowName,displayTitle,conclusion,startedAt,updatedAt", *event]))
            if len(samples) == 1000:
                raise ValueError("workflow history truncated")
            history.update((item["databaseId"], item) for item in samples)
        medians[workflow] = (saved["medians"][workflow] if workflow in saved["medians"]
                             else workflow_median(history.values(), now))
        active = json.loads(run(["gh", "run", "list", "--repo", slug, "--workflow", workflow,
                                 "--status", "in_progress", "--limit", "1000",
                                 "--json", "databaseId,workflowName,displayTitle"]))
        for item in active:
            if not landing_run(item):
                continue
            view = json.loads(run(["gh", "run", "view", str(item["databaseId"]), "--repo", slug,
                                   "--json", "jobs"]))
            # gh's jobs projection omits runner_name; the REST jobs endpoint supplies it.
            names = {job["id"]: job.get("runner_name") for job in run_jobs(slug, item["databaseId"])}
            for job in view["jobs"]:
                if job["status"] == "in_progress":
                    jobs.append({"workflow": workflow, "run_id": item["databaseId"], "name": job["name"],
                                 "started_at": timestamp(job["startedAt"]),
                                 "runner": names.get(job["databaseId"]) or "waiting"})
    if medians != saved["medians"]:
        median_path.parent.mkdir(parents=True, exist_ok=True)
        median_path.write_text(json.dumps({"epoch": now, "medians": medians}) + "\n")
    return jobs, medians


def measure(repo, now):
    row = {"repo": repo, "epoch": now, "target": project_targets().get(repo), "pending": "error", "landed_per_hour": "error",
           "push_to_land_minutes": "error", "push_to_land_p90_minutes": "error",
           "main_age_minutes": "error", "oldest_job": "error", "stall": "error",
           "wisp": None, "reuse": reuse_verdict(None), "pins": pins_verdict(None),
           **{field: "error" for field in THROUGHPUT}}
    row["goal_finish"] = goal_finish(repo, row["target"])
    path = Path.home() / "code" / repo / "main"
    url = run(["git", "remote", "get-url", "origin"], path)
    slug = github_repo(url) if url else None
    if not slug:
        return row
    if repo == WISP_REPO:
        row["wisp"] = wisp_row(slug, now)
    fetched = run(["git", "fetch", "--quiet", "--prune", "--no-write-fetch-head", "origin",
                   "+refs/heads/main:refs/remotes/origin/main",
                   "+refs/heads/claude/land-*:refs/remotes/origin/claude/land-*"], path)
    if fetched is None:
        return row
    row["pins"] = pins_verdict(pin_builds(path))
    log = run(["git", "log", "--first-parent", "--since-as-filter=" + str(int(now - 86400)),
               "--format=%H %ct", "origin/main"], path)
    config = run(["git", "config", "--blob", "origin/main:.safe-push", "--get", "landing.queue"], path)
    autoland = config and config.strip() == "autoland"
    pushes = landed = None
    if log is not None:
        commits = [(sha, int(at)) for sha, at in (line.split() for line in log.splitlines())]
        row["landed_per_hour"] = sum(at >= now - 10800 for _, at in commits) / 3
        if autoland:
            median, p90, reuse, pushes, landed = autoland_latency(slug, now)
            row["reuse"] = reuse_verdict(reuse)
        else:
            median, p90 = latency(path, slug, commits)
            pushes = local_push_times(slug)
        row["push_to_land_minutes"] = median
        row["push_to_land_p90_minutes"] = p90
    branches = run(["git", "for-each-ref", "--format=%(objectname) %(committerdate:unix) %(refname:lstrip=3)",
                    "refs/remotes/origin/claude/land-*"], path)
    if branches is None:
        return row
    tips = {sha: (int(at), name) for sha, at, name in (line.split(" ", 2) for line in branches.splitlines())}
    listed = run(["git", "for-each-ref", "--no-merged=origin/main", "--format=%(objectname)",
                  "refs/remotes/origin/claude/land-*"], path)
    open_tips = set(listed.split()) if listed is not None else None
    unlanded = [sha for sha in tips
                if (sha in open_tips if open_tips is not None
                    else run(["git", "merge-base", "--is-ancestor", sha, "origin/main"], path) is None)]
    states = autoland_states(slug, unlanded) if autoland else {}
    pending, row["dead"], row["dead_oldest_minutes"] = lane_counts(unlanded, states, now)
    dead = {sha for sha in unlanded if sha in states and states[sha]["state"] in ("FAILURE", "ERROR")}
    history = run(["git", "log", "--first-parent", "--since-as-filter=" + str(int(now - 7 * 86400)),
                   "--format=%H%x1f%ct%x1f%B%x1e", "origin/main"], path)
    open_count, closed = issue_activity(slug, now)
    if log is not None and history is not None and closed is not None and pushes is not None:
        main_log = [(sha, int(at), message) for sha, at, message in
                    (entry.strip("\n").split("\x1f", 2) for entry in history.split("\x1e") if entry.strip())]
        lanes = [(timestamp(states[sha]["createdAt"]),
                  issue_refs(run(["git", "log", "--format=%B", "origin/main.." + sha], path) or "", repo)
                  | {int(n) for n in re.findall(r"(?:^|[-_/#])(\d+)(?=$|[-_/])", tips[sha][1])})
                 for sha in dead]
        row.update(throughput(repo, now, open_count, closed, main_log,
                              landed if landed is not None else [at for _, at in commits], pushes,
                              project_targets(), lanes, claim_times(now).get(repo, {}),
                              [tips[sha][0] for sha in unlanded if sha not in dead]))
    if autoland:
        for status in ("queued", "in_progress"):
            raw = run(["gh", "run", "list", "--repo", slug, "--workflow", "autoland.yml",
                       "--status", status, "--limit", "1000", "--json", "displayTitle"])
            try:
                pending += sum(run["displayTitle"].startswith("Autoland test ") for run in json.loads(raw))
            except (ValueError, TypeError, KeyError, AttributeError):
                return row
    row["pending"] = pending
    tip = run(["git", "log", "-1", "--format=%H %ct", "origin/main"], path)
    if log is None or not tip:
        return row
    try:
        sha, at = tip.split()
        row["main_sha"] = sha
        row["main_age_minutes"] = max(0, now - int(at)) / 60
        jobs, medians = landing_jobs(path, slug, now)
        row["oldest_job"] = ({**min(jobs, key=lambda job: job["started_at"]),
                              "age_minutes": max(0, now - min(job["started_at"] for job in jobs)) / 60}
                             if jobs else None)
        row["stall"] = stall_verdict(now, int(at), [at for _, at in commits], pending, jobs, medians)
    except (ValueError, TypeError, KeyError, AttributeError):
        pass
    return row


THROUGHPUT = ("closed_per_hour", "needed_per_hour", "land_to_close_minutes", "land_ratio_3h",
              "failed_unowned", "oldest_lane_wait_minutes")


def get(repo, now=None, fields=("reuse", "closed_per_hour")):
    if not valid_name(repo):
        raise ValueError("invalid repository name")
    now = time.time() if now is None else now
    row = cached(repo, now, fields)
    if row is not None and row.get("target") == project_targets().get(repo):
        return row
    row = measure(repo, now)
    path = cache_path(repo)
    try:
        path.parent.mkdir(parents=True, exist_ok=True)
        temporary = path.with_suffix(f".{os.getpid()}.tmp")
        temporary.write_text(json.dumps(row) + "\n")
        temporary.replace(path)
    except OSError:
        pass
    return row


def format_row(row):
    rate = row["landed_per_hour"]
    median = row["push_to_land_minutes"]
    p90 = row["push_to_land_p90_minutes"]
    rate = f"{rate:g}" if isinstance(rate, (int, float)) else rate
    median = "n/a" if median is None else f"{median:g}m" if isinstance(median, (int, float)) else median
    p90 = "n/a" if p90 is None else f"{p90:g}m" if isinstance(p90, (int, float)) else p90
    dead = (f", dead {row['dead']} (oldest {row['dead_oldest_minutes']:.0f}m)"
            if row.get("dead") and isinstance(row.get("dead_oldest_minutes"), (int, float)) else "")
    text = (f"{row['repo']}: pending {row['pending']}{dead}, landed {rate}/h over 3h, "
            f"push-to-land median {median} p90 {p90} over 24h")
    if "stall" in row:
        age = row["main_age_minutes"]
        age = f"{age:.1f}m" if isinstance(age, (int, float)) else age
        job = row["oldest_job"]
        oldest = (f"{job['age_minutes']:.1f}m {job['workflow']} run {job['run_id']} "
                  f"{job['name']} on {job['runner']}" if isinstance(job, dict) else "none" if job is None else job)
        verdict = row["stall"]
        verdict = (("STALLED: " if verdict["stalled"] else "") + verdict["reason"]
                   if isinstance(verdict, dict) else verdict)
        text += f", main idle {age}, oldest landing job {oldest}, stall {verdict}"
    if "closed_per_hour" in row:
        def show(value, digits, unit=""):
            return "n/a" if value is None else f"{value:.{digits}f}{unit}" if isinstance(value, (int, float)) else value
        text += (f", closed {show(row['closed_per_hour'], 1, '/h')} needed {show(row['needed_per_hour'], 1, '/h')}, "
                 f"land-to-close {show(row['land_to_close_minutes'], 0, 'm')}, "
                 f"land ratio {show(row['land_ratio_3h'], 2)} over 3h, failed unowned {row['failed_unowned']}, "
                 f"oldest lane wait {show(row['oldest_lane_wait_minutes'], 0, 'm')}")
    text += "".join(f", {guard['text']}" + (" FLAGGED" if guard["reason"] else "")
                    for guard in (row.get("actions"), row.get("wisp"), row.get("reuse"), row.get("pins")) if guard)
    finish = row.get("goal_finish")
    if row.get("target") is not None and isinstance(finish, (int, float)) and finish > row["target"]:
        target = datetime.fromtimestamp(row["target"], timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
        predicted = datetime.fromtimestamp(finish, timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
        text += f", goal ETA finish {predicted}\n" + missed_target(row["repo"], target)
    return text


def goal_finish(repo, target, rows=None):
    """Epoch of the newest goal_eta prediction (agents plan's critical path over the goal's own boxes) recorded for
    this repo's current target, or None."""
    import calibration
    if target is None:
        return None
    rows = calibration.rows() if rows is None else rows
    best = None
    for row in rows:
        try:
            detail = json.loads(row["source"])
            if row["kind"] == "goal_eta" and row["cell"] == repo and timestamp(detail["target"]) == target \
                    and (best is None or row["at"] > best[0]):
                best = (row["at"], timestamp(detail["predicted_finish"]))
        except (ValueError, KeyError, TypeError):
            continue
    return best[1] if best else None


def guard_reasons(row):
    return [(kind, row[kind]["reason"]) for kind in ("actions", "wisp", "reuse", "pins")
            if isinstance(row.get(kind), dict) and row[kind]["reason"]]


def alert_plan(rows, previous, now, nodes):
    state = json.loads(json.dumps(previous))
    actions = []
    for row in rows:
        repo = row["repo"]
        verdict = row["stall"]
        if not isinstance(verdict, dict):
            continue
        guards = guard_reasons(row)
        if not verdict["stalled"] and not guards:
            if repo in state and row["main_sha"] != state[repo]["main_sha"]:
                del state[repo]
            continue
        cwd_fragment = f"/code/{repo}/"
        parent = next((node.get("parent") or node["id"] for node in nodes
                       if node["state"] == "active" and cwd_fragment in node.get("cwd", "")), f"{repo}-parent")
        record = state.setdefault(repo, {"since": now, "main_sha": row["main_sha"], "sent": []})
        for kind, reason in guards:
            for node in (parent, "proxy"):
                channel = "fleet"
                key = f"guard:{kind}:{channel}:{node}"
                if key not in record["sent"]:
                    actions.append({"repo": repo, "key": key, "node": node, "channel": channel,
                                    "text": f"{'Broken pin' if kind == 'pins' else 'Underused capacity'} in {repo}: {reason}"})
        if not verdict["stalled"]:
            continue
        text = f"Landing stalled in {repo}: {verdict['reason']}"
        if verdict["runner"]:
            text += f"; oldest landing job runner {verdict['runner']}"
        for node in (parent, "proxy"):
            channel = "fleet"
            key = f"notice:{channel}:{node}"
            if key not in record["sent"]:
                actions.append({"repo": repo, "key": key, "node": node, "channel": channel, "text": text})
    return state, actions


def fleet_nodes():
    result = subprocess.run(["north", "fleet", "list", "--json"], check=False, capture_output=True, text=True)
    if result.returncode:
        raise OSError("Fleet session list unavailable")
    rows = json.loads(result.stdout)
    return [{"id": row["name"], "session": row["conversation"], "parent": row.get("parent"),
             "provider": row["provider"], "state": "finished" if row["status"] == "exited" else "active",
             "pid": row.get("host_pid"), "cwd": row.get("cwd", "")} for row in rows]


def untranscribed(nodes):
    names = []
    for node in nodes:
        if node.get("provider") != "claude" or node.get("state") == "finished" or not node.get("pid"):
            continue
        try:
            environ = (Path("/proc") / str(node["pid"]) / "environ").read_bytes().split(b"\0")
        except OSError:
            continue
        if any(item.startswith(b"CLAUDE_CODE_CHILD_SESSION=") for item in environ):
            names.append(node["id"])
    return names


def unnamed(nodes):
    sessions = Path(os.environ.get("CLAUDE_CONFIG_DIR") or Path.home() / ".claude") / "sessions"
    named = {}
    for path in sessions.glob("*.json"):
        try:
            record = json.loads(path.read_text())
        except (OSError, ValueError):
            continue
        named[record.get("sessionId")] = record.get("name")
    names = []
    for node in nodes:
        if node.get("provider") != "claude" or node.get("state") == "finished" or not node.get("pid"):
            continue
        if not (Path("/proc") / str(node["pid"])).exists():
            continue
        if named.get(node.get("session")) != node["id"]:
            names.append(node["id"])
    return names


def usage_stale(now, hours=7):
    path = state_dir() / "agents/usage/claude-web.json"
    try:
        age = (now - path.stat().st_mtime) / 3600
    except OSError:
        return None
    return age if age > hours else None


def stall_alert(rows, now):
    directory = state_dir() / "agents/flow"
    directory.mkdir(parents=True, exist_ok=True)
    path = directory / "stalls.json"
    with (directory / "stalls.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        previous = json.loads(path.read_text()) if path.exists() else {}
        nodes = fleet_nodes()
        state, actions = alert_plan(rows, previous, now, nodes)
        for action in actions:
            if not valid_name(action["node"]):
                raise ValueError("invalid Fleet session name")
            if run(["north", "fleet", "send", action["node"], action["text"]]) is None:
                print(f"{action['repo']}: Fleet notification error for {action['node']}")
                continue
            state[action["repo"]]["sent"].append(action["key"])
            temporary = path.with_suffix(f".{os.getpid()}.tmp")
            temporary.write_text(json.dumps(state) + "\n")
            temporary.replace(path)
        temporary = path.with_suffix(f".{os.getpid()}.tmp")
        temporary.write_text(json.dumps(state) + "\n")
        temporary.replace(path)


def brief_coverage():
    from calibration import ledger_path
    try:
        with open_ledger(ledger_path()) as db:
            rows = db.execute("SELECT category,spec,scope,surface,verify,brief_text,brief_origin FROM runs"
                              " WHERE role='worker' AND outcome != 'running'").fetchall()
        import runpy
        contract = runpy.run_path(str(Path(__file__).resolve().parents[2] / "bin/threads"))
        covered = 0
        for category, spec, scope, surface, verify, text, origin in rows:
            fields = dict(category=category, spec=spec, scope=scope, surface=surface, verify=verify)
            declared = contract["brief_fields"](text or "")
            if (category in contract["CATEGORIES"] and all(fields[key] in values for key, values in contract["FACETS"].items())
                    and all(declared.get(key) == value for key, value in fields.items())
                    and origin in ("codex.PostToolUse.spawn_agent", "claude.PostToolUse.Agent")):
                covered += 1
        return covered
    except (OSError, sqlite3.Error):
        return None


def main():
    parser = argparse.ArgumentParser(prog="agents flow")
    parser.add_argument("--json", action="store_true")
    parser.add_argument("--repo", action="append")
    parser.add_argument("--stall-alert", action="store_true")
    args = parser.parse_args()
    repos = list(dict.fromkeys(args.repo or (*DEFAULT_REPOS, *project_targets())))
    if any(not valid_name(repo) for repo in repos):
        parser.error("invalid repository name")
    now = time.time()
    rows = [get(repo, now) for repo in repos]
    samples = account_busy(now)
    for row in rows:
        row["actions"] = actions_verdict(row["pending"], samples, now)
    if not args.json:
        print(laptop_line())
    print(json.dumps(rows) if args.json else "\n".join(map(format_row, rows)))
    nodes = fleet_nodes()
    stale = untranscribed(nodes)
    if stale and not args.json:
        print(f"agents flow: no transcript (inherited CLAUDE_CODE_CHILD_SESSION), restart at a break: {', '.join(stale)}")
    usage_age = usage_stale(now)
    if usage_age is not None and not args.json:
        print(f"agents flow: usage cache stale: claude-web.json last written {usage_age:.0f}h ago; usage-web-read runs every 6h")
    nameless = unnamed(nodes)
    if nameless and not args.json:
        print(f"agents flow: not reachable by Fleet name, restart at a break: {', '.join(nameless)}")
    if not args.json:
        covered = brief_coverage()
        print(f"agents flow: tagged finished worker briefs {covered if covered is not None else 'unknown'}/300 retrieval trigger; backtest requires >500")
    if args.stall_alert:
        try:
            stall_alert(rows, now)
        except (OSError, ValueError, KeyError, TypeError):
            print("agents flow: stall alert state error")
        diagnose = Path(__file__).resolve().parents[2] / "bin/ci-diagnose"
        try:
            subprocess.run([str(diagnose)], timeout=1200, check=False)
        except (OSError, subprocess.TimeoutExpired):
            print("agents flow: ci-diagnose error")
    print(f"agents flow: {len(CALLS)} GitHub API calls this run", file=sys.stderr)


if __name__ == "__main__":
    main()
