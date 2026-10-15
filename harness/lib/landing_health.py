import json
import os
import re
import subprocess
import time
from functools import partial
from datetime import datetime
from pathlib import Path

REFRESH = 120
TOTAL = 90


def command(argv, deadline=None):
    timeout = 5 if deadline is None else min(5, deadline - time.monotonic())
    if timeout <= 0:
        raise TimeoutError("landing-health source deadline exhausted")
    result = subprocess.run(argv, capture_output=True, text=True, timeout=timeout)
    if result.returncode:
        raise RuntimeError(result.stderr.strip() or f"{argv[0]} exited {result.returncode}")
    return result.stdout.strip()


def instant(value):
    return datetime.fromisoformat(value.replace("Z", "+00:00")).timestamp()


def git(path, *args, deadline=None):
    return command(["git", "-C", str(path), *args], deadline)


def activity(repository, now, deadline=None):
    rows = []
    for page in range(1, 6):
        batch = json.loads(command(["gh", "api", f"repos/{repository}/activity?time_period=day&per_page=100&page={page}"], deadline))
        if not isinstance(batch, list):
            raise ValueError("repository activity is not a list")
        for row in batch:
            if not isinstance(row, dict) or not all(isinstance(row.get(field), str) for field in ("ref", "before", "after", "timestamp")):
                raise ValueError("repository activity schema incomplete")
            row["at"] = instant(row["timestamp"])
        rows.extend(batch)
        if len(batch) < 100 or min(row["at"] for row in batch) < now - 3600:
            return rows
    raise RuntimeError("repository activity exceeds five-page coverage budget")


def generated(path, deadline=None):
    read = partial(git, path, deadline=deadline)
    names = read("ls-tree", "-r", "--name-only", "origin/main").splitlines()
    attributes = read("show", "origin/main:.gitattributes") if ".gitattributes" in names else ""
    authority = {line.split()[0] for line in attributes.splitlines()
                 if line.strip() and not line.startswith("#") and any("generated" in value for value in line.split()[1:])}
    if ".safe-push" in names:
        config = read("config", "--blob", "origin/main:.safe-push", "--null", "--list")
        for entry in config.split("\0"):
            key = entry.split("\n", 1)[0]
            if key.startswith("generated.") and key.endswith(".input"):
                authority.add(key.removeprefix("generated.").removesuffix(".input"))
    declaration = "ts/scripts/wisp/cueModels.ts"
    writer = "ts/scripts/cueModels.ts"
    if declaration in names and writer in names:
        source = read("show", f"origin/main:{declaration}")
        caller = read("show", f"origin/main:{writer}")
        match = re.search(r"export const CUE_MODEL_RECORD\s*=\s*['\"]([^'\"]+)['\"]", source)
        if not match or "CUE_MODEL_RECORD" not in caller or "Bun.write(record," not in caller:
            raise ValueError("cue-model generated record provenance changed")
        authority.add(match[1])
    return authority.intersection(names)


def sample(path, previous, now, deadline=None):
    read = partial(git, path, deadline=deadline)
    remote = read("remote", "get-url", "origin")
    match = re.fullmatch(r"(?:https://github.com/|git@github.com:|ssh://git@github.com/)(tompassarelli/[^/]+?)(?:\.git)?", remote)
    if not match:
        raise ValueError("repository origin is not a GitHub repository")
    read("fetch", "-q", "--prune", "origin", "+refs/heads/*:refs/remotes/origin/*")
    main = read("rev-parse", "origin/main")
    refs = read("for-each-ref", "--format=%(refname:short) %(objectname)", "--no-merged=origin/main", "refs/remotes/origin/claude/land-*")
    authority = generated(path, deadline)
    rows = activity(match[1], now, deadline) if refs or authority else []
    main_rows = [row for row in rows if row["ref"] == "refs/heads/main"]
    if main_rows and max(main_rows, key=lambda row: row["at"])["after"] != main:
        raise RuntimeError("repository activity and fetched main tip disagree; arrival coverage unknown")
    lanes = {}
    for line in refs.splitlines():
        name, tip = line.split()
        ref = "refs/heads/" + name.removeprefix("origin/")
        seen = previous.get("lanes", {}).get(ref, now - 3600)
        history = [row for row in rows if row["ref"] == ref]
        creation = max([row["at"] for row in history if (row.get("activity_type") == "branch_creation" or row["before"] == "0" * 40) and row["after"] != "0" * 40], default=0)
        seen = max(seen, creation)
        evidence = [row["at"] for row in history if row["at"] >= creation and row["after"] != "0" * 40]
        lanes[ref] = min([seen, *evidence])
    arrivals = {}
    for row in sorted(rows, key=lambda row: row["at"]):
        if row["ref"] != "refs/heads/main" or row["at"] < now - 3600:
            continue
        before, after = row["before"], row["after"]
        if not re.fullmatch(r"[0-9a-f]{40}", before) or not re.fullmatch(r"[0-9a-f]{40}", after):
            raise ValueError("main activity has invalid graph endpoints")
        if int(after, 16) == 0:
            raise ValueError("main was deleted during observation window")
        arguments = [after] if int(before, 16) == 0 else [f"{before}..{after}"]
        reached = set(read("rev-list", *arguments).splitlines())
        current = [main] if int(before, 16) == 0 else [main, "^" + before]
        projected = set(read("rev-list", *current).splitlines())
        for sha in reached.intersection(projected):
            arrivals.setdefault(sha, row["at"])
    if len(arrivals) > 1000:
        raise RuntimeError("main arrivals exceed bounded graph measurement")
    changes = {}
    if arrivals and authority:
        output = read("show", "--format=commit:%H", "--numstat", "--diff-merges=first-parent", *arrivals)
        sha = None
        for line in output.splitlines():
            if line.startswith("commit:"):
                sha = line.removeprefix("commit:")
            elif sha and "\t" in line:
                added, removed, name = line.split("\t", 2)
                if name in authority and added.isdigit() and removed.isdigit() and int(added) + int(removed) > 800:
                    changes.setdefault(name, set()).add(sha)
    return dict(at=now, main=main, lanes=lanes, arrivals=arrivals,
                changes={name: sorted(shas) for name, shas in changes.items()})


def anomalies(observation, now):
    old = sum(now - at >= 1800 for at in observation["lanes"].values())
    landed = sum(at >= now - 1800 for at in observation["arrivals"].values())
    found = {}
    if old >= 4 and landed <= 1:
        found["LANDING-STALL"] = f"{old} unlanded land refs at least30m old; {landed} commits actually reached current origin/main within30m"
    for name, shas in observation["changes"].items():
        recent = [sha for sha in shas if observation["arrivals"][sha] >= now - 3600]
        if len(recent) >= 2:
            found[f"REGEN-CHURN:{name}"] = f"generated {name} changed more than800 lines in {len(recent)} commits actually reached current origin/main within1h"
    return found


def supervise(store, registry, directory, now, root=None, observe=sample):
    import events
    started = time.monotonic()
    root = Path(os.environ.get("AGENTS_REPO_ROOT", Path.home() / "code")) if root is None else Path(root)
    state_path = Path(directory) / "landing-health.json"
    try:
        state = json.loads(state_path.read_text())
    except FileNotFoundError:
        state = {}
    deadline = started + TOTAL
    candidates = sorted(path for path in root.iterdir() if (path / "main/.git").exists())
    repositories = {}
    for index, path in enumerate(candidates):
        discovery_deadline = time.monotonic() + max(0, deadline - time.monotonic()) / (len(candidates) - index) / 3
        try:
            remote = git(path / "main", "remote", "get-url", "origin", deadline=discovery_deadline)
            if re.fullmatch(r"(?:https://github.com/|git@github.com:|ssh://git@github.com/)tompassarelli/[^/]+?(?:\.git)?", remote):
                repositories[path.name] = []
        except (OSError, ValueError, RuntimeError, subprocess.SubprocessError):
            repositories[path.name] = []
    for node in registry.values():
        domain = node.get("domain", "")
        if node.get("role") == "lead" and domain in repositories and node.get("state") == "active":
            repositories[domain].append(node["id"])
    ordered = sorted(repositories, key=lambda name: (state.get(name, {}).get("attempted_at", 0), name))
    for index, name in enumerate(ordered):
        recipients = repositories[name]
        share = max(0, deadline - time.monotonic()) / (len(ordered) - index)
        repository_deadline = time.monotonic() + share
        source_deadline = time.monotonic() + share * 2 / 3
        delivery_store = events.Store(deadline=repository_deadline)
        measured = now + time.monotonic() - started
        record = state.setdefault(name, {})
        refresh = measured - record.get("attempted_at", 0) >= REFRESH
        try:
            if not refresh:
                if record.get("source_error"):
                    raise RuntimeError(record["source_error"])
                observation = record["observation"]
            else:
                record["attempted_at"] = measured
                observation = observe(root / name / "main", record.get("observation", {}), measured, deadline=source_deadline)
                if time.monotonic() > source_deadline:
                    raise TimeoutError("landing-health source deadline exhausted")
            measured = now + time.monotonic() - started
            found = anomalies(observation, measured)
            if not recipients:
                found["LANDING-HEALTH-OWNER"] = "no registered active owning lead; proxy owns routing the observed repository signals"
            record["observation"] = observation
            record["source_error"] = None
        except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
            record["source_error"] = str(error)
            found = {"LANDING-HEALTH-SOURCE": f"source unavailable: {error}; landing throughput and regeneration health unknown"}
        episodes = record.setdefault("episodes", {})
        for identity, episode in episodes.items():
            if identity not in found and (not record["source_error"] or identity == "LANDING-HEALTH-SOURCE"):
                episode["active"] = False
        for identity, body in found.items():
            episode = episodes.setdefault(identity, {"serial": 0, "active": False, "sent": []})
            if not episode["active"]:
                episode.update(serial=episode["serial"] + 1, active=True, sent=[],
                               body=f"{identity.split(':', 1)[0]} {name}: {body}")
            episode.setdefault("body", f"{identity.split(':', 1)[0]} {name}: {body}")
            for recipient in (["proxy"] if identity == "LANDING-HEALTH-SOURCE" else [*recipients, "proxy"]):
                if recipient in episode["sent"]:
                    continue
                try:
                    row = delivery_store.publish("QUESTION", from_node="landing-health", to_node=recipient, repo=name,
                                        body=episode["body"],
                                        key=f"landing-health:{name}:{identity}:{episode['serial']}:{recipient}")
                    if not row.get("deferred"):
                        events.mirror(row, registry, deadline=repository_deadline)
                        if events.process(delivery_store, row, registry, measured,
                                          send=lambda node, body: events.deliver(node, body, deadline=repository_deadline)):
                            episode["sent"].append(recipient)
                except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
                    record["delivery_error"] = str(error)
    temporary = state_path.with_suffix(f".{os.getpid()}.tmp")
    temporary.write_text(json.dumps(state, sort_keys=True) + "\n")
    os.replace(temporary, state_path)
