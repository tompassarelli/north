import argparse
from contextlib import contextmanager
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import shlex
import sys
import time

sys.path.insert(0, str(Path(__file__).resolve().parent))
import org
import placement
import resident_cap
import pace_sampler
import spawn_queue
from defaults import DEFAULTS
from usage import local_account


class ReleaseDeadline(BaseException):
    pass


@contextmanager
def registry_until(deadline=None):
    registry = org.Org()
    path = Path(org.org_file())
    path.parent.mkdir(parents=True, exist_ok=True)
    with Path(str(path) + ".lock").open("a") as lock:
        while True:
            try:
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                break
            except BlockingIOError:
                remaining = float("inf") if deadline is None else deadline - time.monotonic()
                if remaining <= 0:
                    raise TimeoutError("spawn release deadline reached")
                time.sleep(min(0.01, remaining))
        try:
            registry.data = json.loads(path.read_text())
        except FileNotFoundError:
            registry.data = {"version": org.VERSION}
        if registry.data.get("version") != org.VERSION:
            org.die(f"{path} has unsupported schema version")
        registry.nodes = registry.data.get("nodes", [])
        yield registry


def owner_start(pid):
    try:
        return Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()[19]
    except FileNotFoundError:
        return None


def claim_path(state, ident):
    return state / "spawn-claims" / ident


def mark(path, value):
    with path.open("a") as target:
        target.write(value + "\n")
        target.flush()
        os.fsync(target.fileno())


def reconcile(state, registry, job, deadline=None):
    ident = job["spawn_id"]
    path = claim_path(state, ident)
    if not path.exists():
        node = registry.find(f"spawn-{ident}")
        return bool(node and (node.get("session") or (node.get("pid") and org.alive(node["pid"]))))
    lines = path.read_text().splitlines()
    if "started" in lines:
        return True
    if not lines:
        return None
    owner = json.loads(lines[0])
    if owner_start(owner["pid"]) == owner["start"]:
        return None
    if "failed" not in lines:
        from events import Store
        node = registry.find(f"spawn-{ident}")
        if node:
            node.update(state="failed", died=time.strftime("%Y-%m-%dT%H:%M:%S%z"))
            registry.save()
        spawn_queue.enqueue_spawn(state, job, {"reason": "launcher exited before worker started"})
        Store(deadline=deadline).publish("STUCK", from_node="spawn-release", to_node="proxy",
                        body=f"Spawn {ident} launcher exited without a worker started mark; queued for recovery",
                        key=f"spawn-failed:{ident}")
        mark(path, "failed")
    return None


def field(text, key):
    match = re.search(rf"(?mi)^\s*{key}:\s*(.+)$", text)
    return match.group(1).strip() if match else ""


def main(argv):
    if len(argv) == 2 and argv[0] == "started":
        mark(Path(argv[1]), "started")
        return 0
    if argv == ["release"]:
        return release()
    parser = argparse.ArgumentParser(prog="agents spawn")
    parser.add_argument("brief", metavar="BRIEF_FILE|ID")
    parser.add_argument("--tier")
    args = parser.parse_args(argv)
    state = Path(os.environ.get("XDG_STATE_HOME") or Path.home() / ".local/state") / "agents"
    parent = os.environ.get("AGENT_ORG_NAME") or "proxy"
    with org.Org() as registry:
        up = registry.find(parent)
        if up is None:
            org.die(f"unknown parent {parent!r}")
        up = dict(up)
    session = os.environ.get("CLAUDE_CODE_SESSION_ID") or os.environ.get("CODEX_THREAD_ID") or up.get("session")
    if not session:
        org.die("spawn needs a parent session id")
    saved = state / "spawn"
    by_id = bool(re.fullmatch(r"[0-9a-f]{12}", args.brief))
    source = saved / f"{args.brief}.md" if by_id else Path(args.brief).expanduser()
    contents = source.read_bytes()
    ident = args.brief if by_id else hashlib.sha256(session.encode() + contents).hexdigest()[:12]
    text = contents.decode("utf-8")
    saved.mkdir(parents=True, exist_ok=True)
    if not (saved / f"{ident}.md").exists():
        (saved / f"{ident}.md").write_bytes(contents)
    provider = up.get("provider", "claude")
    job = {"spawn_id": ident, "parent": parent, "session": session,
           "provider": provider, "account": local_account(provider) or up.get("account") or provider,
           "tier": args.tier or field(text, "Tier") or DEFAULTS["default_tier"], "cwd": os.getcwd()}
    job = next((entry for entry in spawn_queue.read(state, placement.now().timestamp())
                if entry.get("spawn_id") == ident), job)
    result = start(state, job)
    if result == 0:
        path = claim_path(state, ident)
        if path.exists() and "started" not in path.read_text().splitlines():
            return result
        for entry in spawn_queue.read(state, placement.now().timestamp()):
            if entry.get("spawn_id") == ident:
                spawn_queue.admit(state, entry["lane"], ident)
    return result


def start(state, job, decision=None, unattended=False, deadline=None):
    ident, parent, session = job["spawn_id"], job["parent"], job["session"]
    name = f"spawn-{ident}"
    saved = state / "spawn"
    brief = saved / f"{ident}.md"
    text = brief.read_text()
    with registry_until(deadline) as registry:
        existing = reconcile(state, registry, job, deadline)
        if existing is not False:
            print(f"{name} (already active)")
            return 3 if unattended and existing is None else 0
        cfg = placement.config()
        tier = job["tier"]
        if decision is None:
            gate_path = state / "usage-gate.json"
            gate = json.loads(gate_path.read_text()) if gate_path.exists() else {}
            decision = placement.place_spawn(text, job, gate, cfg, placement.now())
        kind = decision["kind"]
        if not unattended and kind in ("native", "launch"):
            lane = decision.get("account") or job["account"]
            if spawn_queue.ahead(state, lane, ident):
                decision = dict(decision, kind="queued", reason="older spawns wait for this account")
                kind = "queued"
        if kind in ("native", "launch") and decision.get("candidates"):
            decision = placement.admit(decision["candidates"], state / "usage-gate.json",
                                       eligible=decision.get("eligible"), session=session)
            kind = decision["kind"]
        if unattended and kind == "native":
            kind = "launch"
        if kind == "native":
            tier = decision.get("tier", tier)
            tiers = cfg.get("tiers", {})
            key = next((k for k, t in tiers.items() if tier in (k, k.split(":", 1)[-1], t.get("agent"))), tier)
            agent = tiers.get(key, {}).get("agent") or key.split(":", 1)[-1]
            print("Agent(" + ", ".join(f"{k}={json.dumps(v)}" for k, v in
                  {"subagent_type": agent, "prompt": text}.items()) + ")")
            return 0
        if kind == "launch" and decision.get("host") not in (None, resident_cap.host()):
            decision = dict(decision, kind="queued", reason=f"host {decision['host']} cannot launch on this machine")
            kind = "queued"
        if kind in ("queued", "refused"):
            if kind == "queued":
                position = spawn_queue.enqueue_spawn(state, job, decision)
                if position:
                    print(spawn_queue.notice(*position, automatic=True), file=sys.stderr)
            print(f"{kind}: {decision['reason']}")
            return 3
        if kind != "launch":
            org.die(f"unknown placement kind {kind!r}")
        item = field(text, "Item") or name
        footer = (f"\n\nWhen finished, report with:\n"
                  f"agents event LAND-READY|STUCK {shlex.quote(item)} --from-node {name} "
                  f"--to-node {shlex.quote(parent)} --body <report>\n"
                  f"Choose LAND-READY or STUCK and supply the report, then run:\n"
                  f"agents org finish {name}\n")
        if not text.endswith(footer):
            with brief.open("a") as target:
                target.write(footer)
        org.resident_hold(str(brief))
        node = org.add(registry, name, {"role": "worker", "budget": 0, "parent": parent,
                       "domain": item.split("#", 1)[0].split()[0], "brief": str(brief),
                       "provider": decision["provider"], "account": decision["account"].split(":", 1)[-1]}, save=False)
        extra = (["-m", decision["model"], "-c", f"model_reasoning_effort={decision['effort']}"]
                 if node["provider"] == "codex" else [])
        node.update(model=decision["model"], effort=decision["effort"], tier=decision["tier"],
                    cwd=job["cwd"], args=extra, parent_session=session, state="launching",
                    spawn_id=ident)
        path = claim_path(state, ident)
        path.parent.mkdir(parents=True, exist_ok=True)
        try:
            descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        except FileExistsError:
            return 3
        with os.fdopen(descriptor, "w") as target:
            target.write(json.dumps({"pid": os.getpid(), "start": owner_start(os.getpid())}) + "\n")
            target.flush()
            os.fsync(target.fileno())
        registry.save()
    previous = {key: os.environ.get(key) for key in ("AGENT_SPAWN_CLAIM", "AGENT_SPAWN_TOOL")}
    os.environ.update(AGENT_SPAWN_CLAIM=str(path), AGENT_SPAWN_TOOL=str(Path(__file__).resolve()))
    try:
        session = org.launch(node, extra, node["cwd"], dry=os.environ.get("AGENTS_SPAWN_DRY") == "1")
    except TimeoutError:
        raise
    except (Exception, SystemExit):
        with registry_until(deadline) as registry:
            current = registry.find(name)
            if current:
                current.update(state="failed", died=time.strftime("%Y-%m-%dT%H:%M:%S%z"))
                registry.save()
        raise
    finally:
        for key, value in previous.items():
            if value is None:
                os.environ.pop(key, None)
            else:
                os.environ[key] = value
    with registry_until(deadline) as registry:
        current = registry.find(name)
        if current:
            if session:
                current.update(state="active", session=session)
                print(org.line(current))
            else:
                current.update(state="failed", died=time.strftime("%Y-%m-%dT%H:%M:%S%z"))
            registry.save()
    if not session:
        org.die(f"{name} failed to start", 1)
    return 0


def release(wait=5):
    import signal
    import time

    state = pace_sampler.state_dir()
    if not (state / "spawn-queue.json").exists():
        return 0

    def timeout(*_):
        raise ReleaseDeadline("spawn release deadline reached; remaining jobs still queued")

    previous = signal.signal(signal.SIGALRM, timeout)
    timer = signal.setitimer(signal.ITIMER_REAL, wait)
    began = time.monotonic()
    deadline = began + wait
    try:
        with (state / "spawn-release.lock").open("a") as lock:
            if not pace_sampler.locked(lock, 0):
                return 0
            for job in sorted(spawn_queue.read(state, time.time()), key=lambda e: e["id"]):
                if not job.get("spawn_id"):
                    continue
                try:
                    with registry_until(deadline) as registry:
                        existing = reconcile(state, registry, job, deadline)
                        orphan = registry.find(job["parent"]) is None
                    if existing:
                        spawn_queue.admit(state, job["lane"], job["spawn_id"])
                        continue
                    if existing is None:
                        continue
                    if orphan:
                        from events import Store
                        Store(deadline=deadline).publish("STUCK", from_node="spawn-release", to_node="proxy",
                                        body=f"Queued spawn {job['spawn_id']} lost parent {job['parent']}",
                                        key=f"spawn-orphan:{job['spawn_id']}")
                        spawn_queue.admit(state, job["lane"], job["spawn_id"])
                        continue
                    brief = (state / "spawn" / f"{job['spawn_id']}.md").read_text()
                    decision = placement.place_spawn(brief, job, pace_sampler.load(state / "usage-gate.json", {}),
                                                     placement.config(), placement.now())
                    if decision["kind"] not in ("launch", "native") or decision.get("host") not in (None, resident_cap.host()):
                        continue
                    if start(state, job, decision, unattended=True, deadline=deadline) == 0:
                        spawn_queue.admit(state, job["lane"], job["spawn_id"])
                        return 0
                except TimeoutError:
                    raise
                except (Exception, SystemExit) as error:
                    print(f"agents spawn release: {job['spawn_id']}: {error}", file=sys.stderr)
        return 0
    except (ReleaseDeadline, TimeoutError, OSError, ValueError, AttributeError) as error:
        print(f"agents spawn release: {error}", file=sys.stderr)
        return 2
    finally:
        signal.setitimer(signal.ITIMER_REAL, 0)
        signal.signal(signal.SIGALRM, previous)
        if timer[0]:
            signal.setitimer(signal.ITIMER_REAL, max(0.001, timer[0] - (time.monotonic() - began)), timer[1])


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except (OSError, ValueError, AttributeError) as error:
        print(f"agents spawn: {error}", file=sys.stderr)
        sys.exit(2)
