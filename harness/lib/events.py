import argparse
import concurrent.futures
import fcntl
import json
import os
import signal
import socket
import subprocess
import sys
import threading
import time
import uuid
from datetime import datetime, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import org
from defaults import DEFAULTS

ROOT = Path(__file__).resolve().parents[2]
KINDS = ("KICKOFF", "STUCK", "LANDED", "QUESTION", "LAND-READY", "RECYCLE-READY", "ANSWER", "LEARN")
ACKED = ("KICKOFF", "QUESTION", "STUCK")
INBOUND = {"crossSessionInbound": "accept"}


def run(argv, timeout=20, deadline=None, **kwargs):
    if deadline is not None:
        timeout = min(timeout, deadline - time.monotonic())
        if timeout <= 0:
            raise TimeoutError("landing-health total deadline exhausted")
    return subprocess.run(argv, capture_output=True, text=True, timeout=timeout, **kwargs)


class Store:
    def __init__(self, deadline=None):
        self.deadline = deadline

    def call(self, *args):
        result = run([str(ROOT / "bin/threads"), *map(str, args)], deadline=self.deadline)
        if result.returncode:
            raise RuntimeError(result.stderr.strip() or f"threads exited {result.returncode}")
        if not result.stdout.strip() and args[0] == "agent-event":
            return {"deferred": True}
        return json.loads(result.stdout)

    def pending(self):
        return self.call("agent-events")

    def update(self, identity, **values):
        return self.call("agent-event-state", identity, "--update-json", json.dumps(values))

    def publish(self, kind, item="", **values):
        args = ["agent-event", kind]
        if item:
            args.append(item)
        values.setdefault("key", str(uuid.uuid4()))
        for name, value in values.items():
            if value is not None and value != "":
                args.extend(["--" + name.replace("_", "-"), str(value)])
        result = self.call(*args)
        if result.get("deferred"):
            return dict(values, kind=kind, item=item, id="deferred", deferred=True)
        return result


def timestamp(value):
    return datetime.fromisoformat(value.replace("Z", "+00:00")).timestamp()


def due(row, now, landed=False, progressed=True, active=True):
    if row.get("answered_by") or (row.get("successor") and row["kind"] != "RECYCLE-READY"):
        return []
    if row["kind"] == "LAND-READY" and landed:
        return ["landed"]
    actions = []
    if not row.get("delivered_at") and now - (row.get("attempted_at") or 0) >= 60:
        actions.append("deliver")
    age = now - timestamp(row["time"])
    ask = not row.get("root_event") or (row.get("key") or "").startswith("reroute:")
    current = timestamp(row["time"]) >= timestamp(DEFAULTS["event_ack_since"])
    ack = DEFAULTS["event_ack_min"] * 60
    if row["kind"] in ACKED and ask and current and not progressed:
        if not active and age >= ack:
            return [] if row.get("parent_escalated_at") else ["parent"]
        if (age >= ack and not row.get("resend_at")
                and (not row.get("last_error") or now - (row.get("attempted_at") or 0) >= 60)):
            if "deliver" in actions:
                actions.remove("deliver")
            actions.append("resend")
        if age >= 2 * ack and not row.get("proxy_escalated_at"):
            actions.append("alarm")
    if (row["kind"] in ("KICKOFF", "QUESTION") and ask and current and not progressed and not row.get("proxy_escalated_at")
            and age >= DEFAULTS["event_ask_progress_min"] * 60 and "alarm" not in actions):
        actions.append("alarm")
    if row.get("root_event"):
        return actions
    if row["kind"] in ("STUCK", "QUESTION") and age >= DEFAULTS["event_stuck_resend_min"] * 60:
        if (not row.get("resend_at") and "resend" not in actions
                and (not row.get("last_error") or now - (row.get("attempted_at") or 0) >= 60)):
            if "deliver" in actions:
                actions.remove("deliver")
            actions.append("resend")
        if row["to_node"] != "proxy" and not row.get("proxy_escalated_at") and "alarm" not in actions:
            actions.append("proxy")
    if row["kind"] == "LAND-READY":
        if age >= DEFAULTS["event_land_page_min"] * 60 and not row.get("parent_escalated_at"):
            actions.append("parent")
        if age >= DEFAULTS["event_land_proxy_min"] * 60 and row["to_node"] != "proxy" and not row.get("proxy_escalated_at"):
            actions.append("proxy")
    if row["kind"] == "RECYCLE-READY" and age >= DEFAULTS["event_recycle_restart_min"] * 60 and not row.get("successor"):
        actions.append("recycle")
    priority = {"alarm": 0, "proxy": 0, "parent": 1, "resend": -1, "deliver": 3, "recycle": 4}
    return sorted(actions, key=priority.get)


def append(path, line, deadline=None):
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a+b") as stream:
        if deadline is not None and time.monotonic() >= deadline:
            raise TimeoutError("event inbox delivery deadline exhausted")
        try:
            fcntl.flock(stream, fcntl.LOCK_EX | (fcntl.LOCK_NB if deadline is not None else 0))
        except BlockingIOError:
            raise TimeoutError("event inbox is locked; retry on the next supervisor pass") from None
        stream.seek(0)
        content = (line + "\n").encode()
        if content in stream.read():
            return
        stream.seek(0, os.SEEK_END)
        if stream.tell():
            stream.seek(-1, os.SEEK_END)
            if stream.read(1) != b"\n":
                stream.write(b"\n")
        stream.write(content)


def refusal(registry, name):
    node = registry.get(name)
    if name == "proxy":
        return None
    if not node:
        return "removed from the org"
    if node.get("state") != "active":
        return f"{node.get('state')} in the org"
    try:
        lines = Path(node.get("status_file") or "").read_text().strip().splitlines()
    except OSError:
        lines = []
    if lines and lines[-1].startswith("FINISHED"):
        return f"FINISHED in {node['status_file']}"
    if org.health(node) != "active":
        return "a dead session"
    return None


def progressed(row, registry):
    owner = registry.get(row["to_node"])
    if not owner or refusal(registry, row["to_node"]):
        return False
    try:
        return any(line.split()[:2] == ["ACK", str(row["id"])]
                   for line in Path(owner.get("status_file") or "").read_text().splitlines())
    except OSError:
        return False


def nodes():
    with org.Org() as registry:
        return {node["id"]: dict(node) for node in [registry.find("proxy"), *registry.nodes]}


def describe(row):
    return (f"EVENT {row['id']} {row['kind']} {row.get('item') or ''} "
            f"from={row['from_node']} to={row['to_node']} "
            f"lane={row.get('lane') or ''} sha={row.get('sha') or ''}\n{row['body']}\n"
            f"Reply with agents event ANSWER --from-node {row['to_node']} "
            f"--to-node {row['from_node']} --answer {row['id']} --body <answer>.")


def inbox_path(node):
    if node["id"] == "proxy":
        return Path(org.org_file()).parent / "handoffs/proxy-inbox.md"
    status = Path(node["status_file"])
    return status.with_name(status.name.removesuffix("-status.md") + "-inbox.md")


def mirror(row, registry, deadline=None):
    source = registry.get(row["from_node"], {})
    target = registry.get(row["to_node"], {})
    line = describe(row)
    if source.get("status_file"):
        append(Path(source["status_file"]), line, deadline=deadline)
    if row["to_node"] == "proxy":
        append(inbox_path({"id": "proxy"}), line, deadline=deadline)
    elif target.get("status_file"):
        append(inbox_path(target), line, deadline=deadline)


def session_address(session, directory=None):
    sockets = Path(directory or os.environ.get("AGENTS_CLAUDE_SOCKET_DIR") or f"/run/user/{os.getuid()}/cc-socks")
    candidates = []
    for path in sorted(sockets.glob("*.sock")) if sockets.is_dir() else []:
        try:
            argv = (Path("/proc") / path.stem / "cmdline").read_bytes().split(b"\0")
        except OSError:
            continue
        if any(arg.split(b"=", 1)[0] == b"--fork-session" for arg in argv):
            continue
        sessions = [argv[index + 1] for index, arg in enumerate(argv[:-1])
                    if arg in (b"--resume", b"--session-id")]
        sessions.extend(arg.split(b"=", 1)[1] for arg in argv
                        if arg.startswith((b"--resume=", b"--session-id=")))
        if sessions == [session]:
            candidates.append(f"uds:{path}")
    return candidates[0] if len(candidates) == 1 else None


def accepts_inbound(pid):
    try:
        argv = (Path("/proc") / str(pid) / "cmdline").read_bytes().split(b"\0")
    except OSError:
        return False
    sources = [argv[index + 1] for index, arg in enumerate(argv[:-1]) if arg == b"--settings"]
    if len(sources) != 1:
        return False
    try:
        settings = json.loads(sources[0])
    except ValueError:
        return False
    return isinstance(settings, dict) and settings.get("crossSessionInbound") == INBOUND["crossSessionInbound"]


def push(node, body, locate=session_address, accepts=accepts_inbound):
    address = locate(node["session"].encode()) if node.get("session") else None
    if not address:
        raise RuntimeError(f"Claude session {node['id']} has no live socket")
    path = Path(address.removeprefix("uds:"))
    if not accepts(path.stem):
        raise RuntimeError(f"Claude session {node['id']} (pid {path.stem}) lacks crossSessionInbound=accept; "
                           "its socket would drop pushed events; relaunch it through agents lead")
    with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as peer:
        peer.settimeout(3)
        peer.connect(str(path))
        peer.sendall((json.dumps({"type": "user", "message": {"role": "user", "content": body}}) + "\n").encode())


def deliver(node, body, locate=session_address, deadline=None, accepts=accepts_inbound):
    if not node or node.get("state") != "active":
        raise RuntimeError("recipient is not an active registered node")
    provider = node.get("provider", "claude")
    if node["id"] == "proxy":
        append(inbox_path(node), body, deadline=deadline)
        if provider == "codex" and org.health(node) != "active":
            raise RuntimeError("registered Codex proxy is unavailable; restore its existing control session")
    if provider == "claude":
        if node["id"] != "proxy":
            if not node.get("status_file"):
                raise RuntimeError("registered Claude session has no inbox")
            append(inbox_path(node), body, deadline=deadline)
        push(node, body, locate, accepts)
        return
    if node["id"] != "proxy" and org.health(node) != "active":
        result = run([str(ROOT / "bin/agents"), "lead", "restart", node["id"]], timeout=80, deadline=deadline)
        if result.returncode:
            raise RuntimeError(result.stderr.strip() or "recipient restart failed")
        node = nodes().get(node["id"])
    session = node.get("session") or ""
    if provider == "codex":
        result = run([str(ROOT / "bin/codex-lead"), "send", body], timeout=30,
                     env=dict(os.environ, CODEX_LEAD_THREAD=session), deadline=deadline)
        if result.returncode:
            raise RuntimeError(result.stderr.strip() or "Codex delivery failed")


def on_main(row):
    if not row.get("lane") or not row.get("sha"):
        return False
    location = row["lane"]
    if not Path(location).is_dir() and row.get("repo"):
        location = str(Path.home() / "code" / row["repo"] / "main")
    fetched = run(["git", "-C", location, "fetch", "-q", "origin", "main"], timeout=10)
    if fetched.returncode:
        return False
    result = run(["git", "-C", location, "merge-base", "--is-ancestor", row["sha"], "origin/main"])
    return result.returncode == 0


def process(store, row, registry, now, send=deliver, landed=on_main, restart=None):
    delivered = bool(row.get("delivered_at"))
    moving = row["kind"] not in ACKED or progressed(row, registry)
    active = not refusal(registry, row["to_node"]) if row["kind"] in ACKED else True
    for action in due(row, now, landed(row) if row["kind"] == "LAND-READY" else False, moving, active):
        if action == "landed":
            store.publish("ANSWER", row.get("item"), from_node=row["to_node"], to_node=row["from_node"],
                          answer=row["id"], body="Recorded SHA is on origin/main", key=f"landed:{row['id']}")
        elif action in ("parent", "proxy", "alarm"):
            receiver = row["to_node"] if action == "parent" else "proxy"
            if action == "parent" and row["kind"] in ACKED:
                parent = registry.get(row["from_node"], {}).get("parent")
                receiver = parent if parent in registry else "proxy"
            body = describe(row)
            if action == "alarm":
                body = (f"ALARM ask {row['id']} has no owner ACK or progress from {row['to_node']} after "
                        f"{int((now - timestamp(row['time'])) // 60)} minutes\n{body}")
            forwarded = store.publish("QUESTION", row.get("item"), from_node=row["from_node"], to_node=receiver,
                                      body=body, root_event=row.get("root_event") or row["id"],
                                      key=f"{action}:{row['id']}", lane=row.get("lane"), sha=row.get("sha"), repo=row.get("repo"))
            store.update(row["id"], **{f"{'proxy' if action == 'alarm' else action}_escalated_at": now})
            if not forwarded.get("deferred"):
                process(store, forwarded, registry, now, send, landed, restart)
        elif action == "recycle":
            try:
                if restart:
                    successor = restart(row)
                else:
                    result = run([str(ROOT / "bin/agents"), "lead", "restart", row["from_node"],
                                  "--packet", row.get("packet") or ""], timeout=90)
                    if result.returncode:
                        raise RuntimeError(result.stderr.strip() or "recycle failed")
                    successor = nodes()[row["from_node"]]["session"]
                store.update(row["id"], successor=successor, last_error=None)
            except (OSError, RuntimeError, subprocess.SubprocessError) as error:
                store.update(row["id"], last_error=str(error))
        elif action in ("deliver", "resend"):
            store.update(row["id"], attempted_at=now)
            try:
                send(registry.get(row["to_node"]), describe(row))
                store.update(row["id"], delivered_at=now, last_error=None,
                             **({"resend_at": now} if action == "resend" else {}))
                delivered = True
            except (OSError, RuntimeError, subprocess.SubprocessError) as error:
                store.update(row["id"], last_error=str(error))
    return delivered


def supervise(once=False):
    directory = Path(org.org_file()).parent
    directory.mkdir(parents=True, exist_ok=True)
    with (directory / "event-supervisor.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        stopped = False

        def stop(*_):
            nonlocal stopped
            stopped = True

        signal.signal(signal.SIGTERM, stop)
        signal.signal(signal.SIGINT, stop)
        store = Store()
        wake = threading.Event()

        def health_loop():
            import landing_health
            while once or not stopped:
                started = time.monotonic()
                try:
                    landing_health.supervise(Store(), nodes(), directory, time.time())
                except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
                    print(f"event-supervisor: {error}", file=sys.stderr, flush=True)
                if once:
                    return
                wake.wait(max(0, DEFAULTS["event_poll_s"] - (time.monotonic() - started)))

        with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
            health = pool.submit(health_loop)
            try:
                while not stopped:
                    started = time.monotonic()
                    try:
                        registry = nodes()
                        groups = {}
                        for row in store.pending():
                            recipient = row["from_node"] if row["kind"] == "RECYCLE-READY" else row["to_node"]
                            groups.setdefault(recipient, []).append(row)

                        def group(rows):
                            for row in rows:
                                if stopped:
                                    break
                                try:
                                    current = store.call("agent-events", "--id", row["id"])[0]
                                    process(store, current, registry, time.time())
                                except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
                                    print(f"event-supervisor: event {row['id']}: {error}", file=sys.stderr, flush=True)

                        list(pool.map(group, groups.values()))
                    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
                        print(f"event-supervisor: {error}", file=sys.stderr, flush=True)
                    if once:
                        break
                    while not stopped and time.monotonic() - started < DEFAULTS["event_poll_s"]:
                        time.sleep(0.2)
            finally:
                stopped = True
                wake.set()
                health.result()


def main(argv=None):
    argv = list(sys.argv[1:] if argv is None else argv)
    if argv[:1] == ["supervise"]:
        if argv[1:] not in ([], ["--once"]):
            raise ValueError("usage: agents supervise [--once]")
        return supervise("--once" in argv)
    if argv[:1] == ["event"]:
        argv.pop(0)
    if argv[:1] == ["list"]:
        if argv[1:]:
            raise ValueError("usage: agents event list")
        print(json.dumps(Store().pending()))
        return
    parser = argparse.ArgumentParser(prog="agents event")
    parser.add_argument("kind", choices=KINDS)
    parser.add_argument("item", nargs="?", default="")
    for name in ("from-node", "to-node", "body", "repo", "lane", "sha", "packet", "key", "answer", "root-event"):
        parser.add_argument("--" + name, required=name == "body")
    args = vars(parser.parse_args(argv))
    registry = nodes()
    session = os.environ.get("CODEX_THREAD_ID") or os.environ.get("CLAUDE_SESSION_ID")
    current = [node for node in registry.values() if session and node.get("session") == session]
    if not args["from_node"] and len(current) == 1:
        args["from_node"] = current[0]["id"]
    if not args["to_node"]:
        source = registry.get(args["from_node"])
        if source and source.get("parent"):
            args["to_node"] = source["parent"]
    if not args["from_node"] or not args["to_node"]:
        parser.error("unregistered session requires explicit --from-node and --to-node")
    if args["kind"] == "RECYCLE-READY" and not Path(args.get("packet") or "").is_file():
        parser.error("RECYCLE-READY requires a readable --packet")
    store = Store()
    kind, item = args.pop("kind"), args.pop("item")
    args["key"] = args["key"] or str(uuid.uuid4())
    refused = None if kind == "ANSWER" else refusal(registry, args["to_node"])
    row = store.publish(kind, item, **args)
    if not refused:
        mirror(row, registry)
        print(json.dumps(row))
        return
    copy = store.publish(kind, item, **dict(args, to_node="proxy", key=f"reroute:{row['key']}",
                                             root_event=None if row.get("deferred") else row["id"]))
    if not row.get("deferred"):
        row = store.update(row["id"], successor="proxy")
    mirror(copy, registry)
    print(json.dumps(row))
    raise RuntimeError(f"refused: {args['to_node']} is {refused}; rerouted to proxy as event {copy['id']}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        sys.exit(f"agents event: {error}")
