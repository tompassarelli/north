import time
from pathlib import Path

import pace_sampler
from defaults import DEFAULTS


def live(entry, now):
    return bool(entry.get("spawn_id")) or now <= max(entry["eta"], entry["seen"]) + DEFAULTS["spawn_queue_lapse_min"] * 60


def read(directory, now):
    return [e for e in pace_sampler.load(Path(directory) / "spawn-queue.json", {}).get("requests", []) if live(e, now)]


def ahead(directory, lane, request, now=None):
    now = time.time() if now is None else now
    entries = [e for e in read(directory, now) if e["lane"] == lane]
    mine = next((e for e in entries if e["request"] == request), None)
    return [e for e in entries if e["request"] != request and (mine is None or e["id"] < mine["id"])]


def update(directory, change, now):
    directory = Path(directory)
    directory.mkdir(parents=True, exist_ok=True)
    with (directory / "spawn-queue.lock").open("a") as lock:
        if not pace_sampler.locked(lock, pace_sampler.RESERVE_WAIT_S):
            return None
        path = directory / "spawn-queue.json"
        state = pace_sampler.load(path, {})
        state["requests"] = [e for e in state.get("requests", []) if live(e, now)]
        result = change(state)
        pace_sampler.save(path, state)
        return result


def enqueue(directory, lane, request, eta, reason, session=None, task=None, now=None, job=None):
    now = time.time() if now is None else now

    def change(state):
        entry = next((e for e in state["requests"] if (job and e.get("spawn_id") == job["spawn_id"]) or
                      (e["lane"] == lane and e["request"] == request)), None)
        if entry is None:
            state["next"] = state.get("next", 0) + 1
            entry = {"id": state["next"], "lane": lane, "request": request, "session": session, "task": task, "at": now}
            state["requests"].append(entry)
        if job and "spawn_id" not in entry:
            entry.update(job)
        entry.update(eta=eta, seen=now, reason=reason)
        lane_entries = sorted((e for e in state["requests"] if e["lane"] == entry["lane"]), key=lambda e: e["id"])
        return lane_entries.index(entry) + 1, entry["eta"]

    return update(directory, change, now)


def admit(directory, lane, request, now=None):
    now = time.time() if now is None else now

    def change(state):
        state["requests"] = [e for e in state["requests"] if not (e["lane"] == lane and e["request"] == request)]

    update(directory, change, now)


def notice(position, eta, automatic=False):
    stamp = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime(eta))
    if automatic:
        return f" Queued #{position}, ETA {stamp}; it starts automatically."
    return (f" Queued #{position}, ETA {stamp}, retry admits it first: wait until then and retry this same spawn; "
            "do not stop or hand back the work.")


def enqueue_spawn(directory, job, decision, now=None):
    from datetime import datetime
    now = time.time() if now is None else now
    eta = decision.get("eta", now + 60)
    if isinstance(eta, str):
        eta = datetime.fromisoformat(eta.replace("Z", "+00:00")).timestamp()
    lane = decision.get("account") or job["account"]
    return enqueue(directory, lane, job["spawn_id"], eta, decision["reason"],
                   session=job["session"], now=now, job=job)
