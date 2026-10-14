import argparse
import concurrent.futures
import fcntl
import json
import os
import signal
import socket
import subprocess
import sys
import time
import uuid
from datetime import datetime, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import org
from defaults import DEFAULTS

ROOT = Path(__file__).resolve().parents[2]
KINDS = ("KICKOFF", "STUCK", "LANDED", "QUESTION", "LAND-READY", "RECYCLE-READY", "ANSWER")


def run(argv, timeout=20, **kwargs):
    return subprocess.run(argv, capture_output=True, text=True, timeout=timeout, **kwargs)


class Store:
    def call(self, *args):
        result = run([str(ROOT / "bin/threads"), *map(str, args)])
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


def due(row, now, landed=False):
    if row.get("answered_by"):
        return []
    if row["kind"] == "LAND-READY" and landed:
        return ["landed"]
    actions = []
    if not row.get("delivered_at") and now - (row.get("attempted_at") or 0) >= 60:
        actions.append("deliver")
    if row.get("root_event"):
        return actions
    age = now - timestamp(row["time"])
    if row["kind"] in ("STUCK", "QUESTION") and age >= DEFAULTS["event_stuck_resend_min"] * 60:
        if not row.get("resend_at"):
            if "deliver" in actions:
                actions.remove("deliver")
            actions.append("resend")
        if row["to_node"] != "proxy" and not row.get("proxy_escalated_at"):
            actions.append("proxy")
    if row["kind"] == "LAND-READY":
        if age >= DEFAULTS["event_land_page_min"] * 60 and not row.get("parent_escalated_at"):
            actions.append("parent")
        if age >= DEFAULTS["event_land_proxy_min"] * 60 and row["to_node"] != "proxy" and not row.get("proxy_escalated_at"):
            actions.append("proxy")
    if row["kind"] == "RECYCLE-READY" and age >= DEFAULTS["event_recycle_restart_min"] * 60 and not row.get("successor"):
        actions.append("recycle")
    priority = {"proxy": 0, "parent": 1, "resend": 2, "deliver": 3, "recycle": 4}
    return sorted(actions, key=priority.get)


def append(path, line):
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("a+b") as stream:
        fcntl.flock(stream, fcntl.LOCK_EX)
        stream.seek(0, os.SEEK_END)
        if stream.tell():
            stream.seek(-1, os.SEEK_END)
            if stream.read(1) != b"\n":
                stream.write(b"\n")
        stream.write((line + "\n").encode())


def nodes():
    with org.Org() as registry:
        return {node["id"]: dict(node) for node in [registry.find("proxy"), *registry.nodes]}


def describe(row):
    return (f"EVENT {row['id']} {row['kind']} {row.get('item') or ''} "
            f"from={row['from_node']} to={row['to_node']} "
            f"lane={row.get('lane') or ''} sha={row.get('sha') or ''}\n{row['body']}\n"
            f"Reply with agents event ANSWER --from-node {row['to_node']} "
            f"--to-node {row['from_node']} --answer {row['id']} --body <answer>.")


def mirror(row, registry):
    source = registry.get(row["from_node"], {})
    target = registry.get(row["to_node"], {})
    line = describe(row)
    if source.get("status_file"):
        append(Path(source["status_file"]), line)
    if target.get("status_file"):
        status = Path(target["status_file"])
        append(status.with_name(status.name.removesuffix("-status.md") + "-inbox.md"), line)
    elif row["to_node"] == "proxy":
        append(Path(org.org_file()).parent / "handoffs/proxy-inbox.md", line)


def deliver(node, body, locate=org.address):
    if not node or node.get("state") != "active":
        raise RuntimeError("recipient is not an active registered node")
    if org.health(node) != "active":
        result = run([str(ROOT / "bin/agents"), "lead", "restart", node["id"]], timeout=80)
        if result.returncode:
            raise RuntimeError(result.stderr.strip() or "recipient restart failed")
        node = nodes().get(node["id"])
    session = node.get("session") or ""
    if node["provider"] == "codex":
        result = run([str(ROOT / "bin/codex-lead"), "send", body], timeout=30,
                     env=dict(os.environ, CODEX_LEAD_THREAD=session))
        if result.returncode:
            raise RuntimeError(result.stderr.strip() or "Codex delivery failed")
    else:
        address = locate(session.encode()) if session else None
        if not address and node.get("pid") and org.alive(node["pid"]):
            directory = Path(f"/run/user/{os.getuid()}/cc-socks")
            address = next((f"uds:{directory / f'{pid}.sock'}" for pid in [node["pid"], *org.descendants(node["pid"])]
                            if (directory / f"{pid}.sock").is_socket()), None)
        if not address:
            raise RuntimeError("registered Claude session has no live socket")
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as peer:
            peer.settimeout(3)
            peer.connect(address.removeprefix("uds:"))
            peer.sendall((json.dumps({"type": "user", "message": {"role": "user", "content": body}}) + "\n").encode())


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
    for action in due(row, now, landed(row) if row["kind"] == "LAND-READY" else False):
        if action == "landed":
            store.publish("ANSWER", row.get("item"), from_node=row["to_node"], to_node=row["from_node"],
                          answer=row["id"], body="Recorded SHA is on origin/main", key=f"landed:{row['id']}")
        elif action in ("parent", "proxy"):
            receiver = "proxy" if action == "proxy" else row["to_node"]
            forwarded = store.publish("QUESTION", row.get("item"), from_node=row["from_node"], to_node=receiver,
                                      body=describe(row), root_event=row.get("root_event") or row["id"],
                                      key=f"{action}:{row['id']}", lane=row.get("lane"), sha=row.get("sha"), repo=row.get("repo"))
            store.update(row["id"], **{f"{action}_escalated_at": now})
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
            store.update(row["id"], attempted_at=now, **({"resend_at": now} if action == "resend" else {}))
            try:
                send(registry.get(row["to_node"]), describe(row))
                store.update(row["id"], delivered_at=now, last_error=None)
            except (OSError, RuntimeError, subprocess.SubprocessError) as error:
                store.update(row["id"], last_error=str(error))


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

                with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
                    list(pool.map(group, groups.values()))
            except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
                print(f"event-supervisor: {error}", file=sys.stderr, flush=True)
            if once:
                break
            while not stopped and time.monotonic() - started < DEFAULTS["event_poll_s"]:
                time.sleep(0.2)


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
    row = Store().publish(args.pop("kind"), args.pop("item"), **args)
    mirror(row, registry)
    print(json.dumps(row))


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, RuntimeError, subprocess.SubprocessError) as error:
        sys.exit(f"agents event: {error}")
