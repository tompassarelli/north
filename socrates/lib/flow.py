import argparse
from datetime import datetime, timezone
import json
import math
import os
import re
import statistics
import subprocess
import time
from pathlib import Path

DEFAULT_REPOS = ("smashcraft", "wisp", "galileo", "north")
CACHE_SECONDS = 300


def state_dir():
    return Path(os.environ.get("XDG_STATE_HOME") or Path.home() / ".local/state")


def github_repo(url):
    match = re.fullmatch(r"(?:https://github\.com/|git@github\.com:|ssh://git@github\.com/)([\w.-]+/[\w.-]+?)(?:\.git)?/?", url.strip())
    return match.group(1) if match else None


def run(argv, cwd=None, input=None):
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


def cached(repo, now):
    try:
        row = json.loads(cache_path(repo).read_text())
        if row["repo"] == repo and "push_to_land_p90_minutes" in row and 0 <= now - row["epoch"] < CACHE_SECONDS:
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
    samples = []
    for tip in tips.values():
        if not tip or tip["oid"] not in pushed or not tip["status"]:
            continue
        for status in tip["status"]["contexts"]:
            if status["context"] != "autoland" or status["state"] != "SUCCESS":
                continue
            landed_sha = re.fullmatch(r"landed as ([0-9a-f]{40,64})", status["description"] or "")
            if not landed_sha:
                continue
            landed = timestamp(status["createdAt"])
            if pushed[tip["oid"]] <= landed <= now:
                samples.append((landed - pushed[tip["oid"]]) / 60)
            break
    return samples


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
            return None, None
        owner, name = slug.split("/")
        objects = " ".join(f'tip{i}: object(oid:{json.dumps(sha)}) {{ ... on Commit {{ '
                           'oid status { contexts { context state description createdAt } } } }'
                           for i, sha in enumerate(shas))
        query = (f'{{ repository(owner:{json.dumps(owner)}, name:{json.dumps(name)}) {{ '
                 + objects + " } }")
        raw = run(["gh", "api", "graphql", "--input", "-"], input=json.dumps({"query": query}))
        response = json.loads(raw)
        if response.get("errors"):
            return "error", "error"
        return percentiles(autoland_samples(pushes, response["data"]["repository"], now))
    except (ValueError, TypeError, KeyError, AttributeError):
        return "error", "error"


def measure(repo, now):
    row = {"repo": repo, "epoch": now, "pending": "error", "landed_per_hour": "error",
           "push_to_land_minutes": "error", "push_to_land_p90_minutes": "error"}
    path = Path.home() / "code" / repo / "main"
    url = run(["git", "remote", "get-url", "origin"], path)
    slug = github_repo(url) if url else None
    if not slug:
        return row
    fetched = run(["git", "fetch", "--quiet", "--prune", "--no-write-fetch-head", "origin",
                   "+refs/heads/main:refs/remotes/origin/main",
                   "+refs/heads/claude/land-*:refs/remotes/origin/claude/land-*"], path)
    if fetched is None:
        return row
    log = run(["git", "log", "--first-parent", "--since-as-filter=" + str(int(now - 86400)),
               "--format=%H %ct", "origin/main"], path)
    config = run(["git", "config", "--blob", "origin/main:.safe-push", "--get", "landing.queue"], path)
    autoland = config and config.strip() == "autoland"
    if log is not None:
        commits = [(sha, int(at)) for sha, at in (line.split() for line in log.splitlines())]
        row["landed_per_hour"] = sum(at >= now - 10800 for _, at in commits) / 3
        median, p90 = autoland_latency(slug, now) if autoland else latency(path, slug, commits)
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
    return row


def get(repo, now=None):
    if not valid_name(repo):
        raise ValueError("invalid repository name")
    now = time.time() if now is None else now
    row = cached(repo, now)
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
    return f"{row['repo']}: pending {row['pending']}, landed {rate}/h over 3h, push-to-land median {median} p90 {p90} over 24h"


def main():
    parser = argparse.ArgumentParser(prog="agents flow")
    parser.add_argument("--json", action="store_true")
    parser.add_argument("--repo", action="append")
    args = parser.parse_args()
    repos = list(dict.fromkeys(args.repo or DEFAULT_REPOS))
    if any(not valid_name(repo) for repo in repos):
        parser.error("invalid repository name")
    rows = [get(repo) for repo in repos]
    print(json.dumps(rows) if args.json else "\n".join(map(format_row, rows)))


if __name__ == "__main__":
    main()
