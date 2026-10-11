import argparse
import fcntl
from datetime import datetime, timezone
import json
import math
import os
import re
import statistics
import subprocess
import sys
import time
from pathlib import Path

DEFAULT_REPOS = ("smashcraft", "wisp", "galileo", "north", "smashcraft-client")
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
WISP_STATUS = ["ssh", "-F", str(Path.home() / ".local/state/smashcraft/vast-launch-check/ssh_config"), "vast",
               'su - user -c "cd /dev/shm/f125/ts && PATH=\\$HOME/.bun/bin:\\$PATH bun scripts/wisp.ts lan status"']
CALLS = []
JOBS = {}


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
        if len(fields) != 4 or fields[1] != slug.split("/")[-1]:
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
    text = f"wisp {len(idle)}/{len(pairs)} pairs idle >{IDLE_SECONDS // 60}m, native-check waiting {waiting}"
    flagged = idle and isinstance(waiting, int) and waiting >= 1
    return {"text": text, "reason": ("idle Wisp pool: " + ", ".join(f"pair {name} idle {seconds / 60:.0f}m"
                                                                  for name, seconds in idle)
                                     + f" with {waiting} native-check waiting") if flagged else None}


def wisp_pool():
    try:
        result = subprocess.run(WISP_STATUS, capture_output=True, text=True, timeout=40, stdin=subprocess.DEVNULL)
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
            return None, None, []
        owner, name = slug.split("/")
        objects = " ".join(f'tip{i}: object(oid:{json.dumps(sha)}) {{ ... on Commit {{ '
                           'oid status { contexts { context state description createdAt } } } }'
                           for i, sha in enumerate(shas))
        query = (f'{{ repository(owner:{json.dumps(owner)}, name:{json.dumps(name)}) {{ '
                 + objects + " } }")
        raw = run(["gh", "api", "graphql", "--input", "-"], input=json.dumps({"query": query}))
        response = json.loads(raw)
        if response.get("errors"):
            return "error", "error", None
        samples, reuse = autoland_samples(pushes, response["data"]["repository"], now)
        return (*percentiles(samples), reuse)
    except (ValueError, TypeError, KeyError, AttributeError):
        return "error", "error", None


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
    row = {"repo": repo, "epoch": now, "pending": "error", "landed_per_hour": "error",
           "push_to_land_minutes": "error", "push_to_land_p90_minutes": "error",
           "main_age_minutes": "error", "oldest_job": "error", "stall": "error",
           "wisp": None, "reuse": reuse_verdict(None), "pins": pins_verdict(None)}
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
    if log is not None:
        commits = [(sha, int(at)) for sha, at in (line.split() for line in log.splitlines())]
        row["landed_per_hour"] = sum(at >= now - 10800 for _, at in commits) / 3
        if autoland:
            median, p90, reuse = autoland_latency(slug, now)
            row["reuse"] = reuse_verdict(reuse)
        else:
            median, p90 = latency(path, slug, commits)
        row["push_to_land_minutes"] = median
        row["push_to_land_p90_minutes"] = p90
    branches = run(["git", "for-each-ref", "--format=%(objectname)",
                    "refs/remotes/origin/claude/land-*"], path)
    if branches is None:
        return row
    pending = 0
    for sha in branches.splitlines():
        contained = run(["git", "merge-base", "--is-ancestor", sha, "origin/main"], path)
        if contained is None:
            pending += 1
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


def get(repo, now=None, fields=("reuse",)):
    if not valid_name(repo):
        raise ValueError("invalid repository name")
    now = time.time() if now is None else now
    row = cached(repo, now, fields)
    if row is not None:
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
    text = (f"{row['repo']}: pending {row['pending']}, landed {rate}/h over 3h, "
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
    return (text
            + "".join(f", {guard['text']}" + (" FLAGGED" if guard["reason"] else "")
                      for guard in (row.get("actions"), row.get("wisp"), row.get("reuse"), row.get("pins")) if guard))


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
        lead = next((node["id"] for node in nodes if repo in (node["domain"], *(node.get("repos") or ()))
                     and node["role"] == "lead" and node["state"] == "active"), f"{repo}-lead")
        record = state.setdefault(repo, {"since": now, "main_sha": row["main_sha"], "sent": []})
        for kind, reason in guards:
            for node, channel in ((lead, "chat"), ("proxy", "chat"), (lead, "inbox")):
                key = f"guard:{kind}:{channel}:{node}"
                if key not in record["sent"]:
                    actions.append({"repo": repo, "key": key, "node": node, "channel": channel,
                                    "text": f"{'Broken pin' if kind == 'pins' else 'Underused capacity'} in {repo}: {reason}"})
        if not verdict["stalled"]:
            continue
        stages = ["notice"] + (["tom"] if now - record["since"] >= 3600 else [])
        for stage in stages:
            text = f"Landing stalled in {repo}: {verdict['reason']}"
            if verdict["runner"]:
                text += f"; oldest landing job runner {verdict['runner']}"
            if stage == "tom":
                text += f"\nNeeds Tom: {repo} has remained stalled for {(now - record['since']) / 60:.0f}m; check landing runner routing and jobs."
            for node, channel in ((lead, "chat"), ("proxy", "chat"), (lead, "inbox")):
                key = f"{stage}:{channel}:{node}"
                if key not in record["sent"]:
                    actions.append({"repo": repo, "key": key, "node": node, "channel": channel, "text": text})
    return state, actions


def stall_alert(rows, now):
    directory = state_dir() / "agents/flow"
    directory.mkdir(parents=True, exist_ok=True)
    path = directory / "stalls.json"
    with (directory / "stalls.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        previous = json.loads(path.read_text()) if path.exists() else {}
        org = Path(os.environ.get("AGENTS_ORG_FILE") or state_dir() / "agents/org.json")
        nodes = json.loads(org.read_text())["nodes"] if org.exists() else []
        state, actions = alert_plan(rows, previous, now, nodes)
        chat = Path(__file__).resolve().parents[2] / "bin/socrates-chat"
        for action in actions:
            if not valid_name(action["node"]):
                raise ValueError("invalid lead name")
            if action["channel"] == "chat":
                if run([str(chat), "post", "--as", action["node"], "--kind", "notice",
                        "--tag", "landing-stall", "-"], input=action["text"]) is None:
                    print(f"{action['repo']}: stall notice error for {action['node']}")
                    continue
            else:
                inbox = state_dir() / "agents/handoffs" / f"{action['node']}-inbox.md"
                inbox.parent.mkdir(parents=True, exist_ok=True)
                stamp = datetime.fromtimestamp(now, timezone.utc).isoformat()
                with inbox.open("a") as stream:
                    stream.write(f"{stamp} {action['text'].replace(chr(10), ' ')}\n")
            state[action["repo"]]["sent"].append(action["key"])
            temporary = path.with_suffix(f".{os.getpid()}.tmp")
            temporary.write_text(json.dumps(state) + "\n")
            temporary.replace(path)
        temporary = path.with_suffix(f".{os.getpid()}.tmp")
        temporary.write_text(json.dumps(state) + "\n")
        temporary.replace(path)


def main():
    parser = argparse.ArgumentParser(prog="agents flow")
    parser.add_argument("--json", action="store_true")
    parser.add_argument("--repo", action="append")
    parser.add_argument("--stall-alert", action="store_true")
    args = parser.parse_args()
    repos = list(dict.fromkeys(args.repo or DEFAULT_REPOS))
    if any(not valid_name(repo) for repo in repos):
        parser.error("invalid repository name")
    now = time.time()
    rows = [get(repo, now) for repo in repos]
    samples = account_busy(now)
    for row in rows:
        row["actions"] = actions_verdict(row["pending"], samples, now)
    print(json.dumps(rows) if args.json else "\n".join(map(format_row, rows)))
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
