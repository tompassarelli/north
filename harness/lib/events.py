import json
import os
import socket
import sqlite3
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import org
from defaults import DEFAULTS

KINDS = ("KICKOFF", "STUCK", "QUESTION", "LAND-READY", "LANDED", "RECYCLE-READY", "ANSWER", "STALL")
WAITING = ("STUCK", "QUESTION", "LAND-READY", "RECYCLE-READY")
USAGE = """usage: agents event KIND ITEM [--lane L] [--sha S] [--to NODE] [--from NODE] [--packet FILE] [--body TEXT]
       agents event list [--open]
       agents supervise [--once]
KIND is one of KICKOFF, STUCK, QUESTION, LAND-READY, LANDED, RECYCLE-READY, ANSWER.
A row goes to the events table and its line to the target's status file (proxy-inbox.md for the proxy).
The target defaults to the caller's parent in agents org; the caller is --from, else $AGENT_ORG_NAME,
else the proxy. A worker sharing its lead's session names its lead with --to.
ANSWER ITEM marks every waiting row for ITEM addressed to the caller answered (the proxy answers any)
and is delivered to the newest such row's sender. LANDED ITEM answers its LAND-READY rows.
RECYCLE-READY needs --packet, the brief its successor starts from.
agents supervise delivers each unanswered row to its target's live Claude socket or Codex thread every
event_poll_s seconds and applies the stall rules in docs/harness.md. Rows live in $AGENTS_EVENTS_DB,
else the threads SQLite on the ledger host, else events.db beside the org registry."""
SCHEMA = """CREATE TABLE IF NOT EXISTS events (
  id INTEGER PRIMARY KEY AUTOINCREMENT, at REAL NOT NULL, repo TEXT NOT NULL DEFAULT '', item TEXT NOT NULL,
  lane TEXT NOT NULL DEFAULT '', sha TEXT NOT NULL DEFAULT '', kind TEXT NOT NULL, from_node TEXT NOT NULL,
  to_node TEXT NOT NULL, body TEXT NOT NULL DEFAULT '', packet TEXT NOT NULL DEFAULT '', about INTEGER,
  delivered_at REAL, attempts INTEGER NOT NULL DEFAULT 0, last_error TEXT NOT NULL DEFAULT '',
  resent_at REAL, paged_at REAL, escalated_at REAL, restarted_at REAL, answered_at REAL,
  answered_by TEXT NOT NULL DEFAULT '')"""


def die(msg, code=2):
    print(f"agents event: {msg}", file=sys.stderr)
    sys.exit(code)


def now():
    return float(os.environ.get("AGENTS_EVENTS_NOW") or time.time())


def db_path():
    if os.environ.get("AGENTS_EVENTS_DB"):
        return Path(os.environ["AGENTS_EVENTS_DB"])
    if not (Path.home() / ".config/threads/remote").exists():
        return Path(os.environ.get("THREADS_DB") or Path.home() / ".local/state/threads/threads.db")
    return Path(org.org_file()).with_name("events.db")


def connect():
    path = db_path()
    path.parent.mkdir(parents=True, exist_ok=True)
    con = sqlite3.connect(path, timeout=30, isolation_level=None)
    con.row_factory = sqlite3.Row
    con.execute("PRAGMA journal_mode=WAL")
    con.execute(SCHEMA)
    return con


def registry():
    try:
        with open(org.org_file(), encoding="utf-8") as f:
            return json.load(f)
    except FileNotFoundError:
        return {"version": org.VERSION}


def nodes(data):
    return {n["id"]: n for n in data.get("nodes", [])}


def handoffs():
    return Path(org.org_file()).with_name("handoffs")


def status_file(name, data):
    node = nodes(data).get(name)
    if node and node.get("status_file"):
        return Path(node["status_file"])
    return handoffs() / ("proxy-inbox.md" if name == "proxy" else f"{name}-inbox.md")


def line(row):
    clock = time.strftime("%H:%M", time.gmtime(row["at"]))
    where = "".join(f" {k} {row[k][:12] if k == 'sha' else row[k]}" for k in ("lane", "sha") if row[k])
    body = f": {row['body']}" if row["body"] else ""
    return f"{row['kind']} {clock}Z {row['item']}{where}{body} (event {row['id']} from {row['from_node']})"


def message(row):
    text = line(row)
    if row["kind"] in WAITING:
        text += f"\nAnswer with: agents event ANSWER {row['item']} --to {row['from_node']} --body '...'"
    return text


def append(path, text):
    path.parent.mkdir(parents=True, exist_ok=True)
    with open(path, "a", encoding="utf-8") as f:
        f.write(text.replace("\n", " ") + "\n")


def insert(con, data, at, kind, item, frm, to, lane="", sha="", body="", packet="", about=None):
    cur = con.execute(
        "INSERT INTO events (at, repo, item, lane, sha, kind, from_node, to_node, body, packet, about)"
        " VALUES (?,?,?,?,?,?,?,?,?,?,?)",
        (at, item.split("#", 1)[0] if "#" in item else "", item, lane, sha, kind, frm, to, body, packet, about))
    row = con.execute("SELECT * FROM events WHERE id=?", (cur.lastrowid,)).fetchone()
    append(status_file(to, data), line(row))
    return row


def answer(con, at, ids, by):
    con.executemany("UPDATE events SET answered_at=?, answered_by=? WHERE id=? AND answered_at IS NULL",
                    [(at, by, i) for i in ids])


def caller(opts):
    return opts.get("from") or os.environ.get("AGENT_ORG_NAME") or "proxy"


def parent(name, data):
    node = nodes(data).get(name)
    return node.get("parent") or "proxy" if node else "proxy"


def write(argv):
    opts, rest = org.parse_opts(list(argv), {"lane", "sha", "to", "from", "body", "packet"})
    if len(rest) != 2 or rest[0] not in KINDS:
        die(USAGE)
    kind, item = rest
    data, at, frm = registry(), now(), caller(opts)
    packet = org.path_of(opts.get("packet", ""))
    if kind == "RECYCLE-READY" and not os.path.isfile(packet):
        die("RECYCLE-READY needs --packet naming the successor's brief file")
    con = connect()
    con.execute("BEGIN IMMEDIATE")
    to = opts.get("to")
    if kind == "ANSWER":
        mine = con.execute(
            f"SELECT id, from_node FROM events WHERE item=? AND answered_at IS NULL AND kind IN ({','.join('?' * len(WAITING))})"
            " AND (to_node=? OR ?='proxy') ORDER BY id", (item, *WAITING, frm, frm)).fetchall()
        answer(con, at, [r["id"] for r in mine], frm)
        to = to or (mine[-1]["from_node"] if mine else None)
    if kind == "LANDED":
        answer(con, at, [r["id"] for r in con.execute(
            "SELECT id FROM events WHERE kind='LAND-READY' AND item=? AND answered_at IS NULL AND (?='' OR lane='' OR lane=?)",
            (item, opts.get("lane", ""), opts.get("lane", "")))], f"LANDED by {frm}")
    row = insert(con, data, at, kind, item, frm, to or parent(frm, data), opts.get("lane", ""), opts.get("sha", ""),
                 opts.get("body", ""), packet)
    con.execute("COMMIT")
    print(line(row))


def listing(argv):
    if argv not in ([], ["--open"]):
        die(USAGE)
    where = "WHERE answered_at IS NULL" if argv else ""
    for row in connect().execute(f"SELECT * FROM events {where} ORDER BY id"):
        state = (f"answered by {row['answered_by']}" if row["answered_at"] else
                 "delivered" if row["delivered_at"] else f"undelivered ({row['attempts']} tries)")
        print(f"{line(row)} -> {row['to_node']} · {state}")


def step(con, at, data, fx):
    """One supervisor pass: stall rules first, then delivery of every undelivered, unanswered row."""
    known = nodes(data)

    def stall(row, to, why):
        insert(con, data, at, "STALL", row["item"], "supervisor", to, row["lane"], row["sha"],
               f"{why}: {row['kind']} from {row['from_node']} to {row['to_node']}: {row['body']}", about=row["id"])

    def mark(row, **cols):
        con.execute(f"UPDATE events SET {', '.join(f'{k}=?' for k in cols)} WHERE id=?", (*cols.values(), row["id"]))

    stuck, land, recycle = (DEFAULTS["event_stuck_resend_min"] * 60, DEFAULTS["event_land_page_min"] * 60,
                            DEFAULTS["event_recycle_restart_min"] * 60)
    for row in con.execute("SELECT * FROM events WHERE answered_at IS NULL AND to_node NOT IN ('proxy', 'supervisor')").fetchall():
        if row["to_node"] not in known:
            answer(con, at, [row["id"]], "supervisor: no such node")
            if row["kind"] in WAITING:
                mark(row, escalated_at=at)
                stall(row, "proxy", f"{row['to_node']} is not in agents org")
    waiting = con.execute(f"SELECT * FROM events WHERE answered_at IS NULL AND kind IN ({','.join('?' * len(WAITING))})"
                          " ORDER BY id", WAITING).fetchall()
    for row in waiting:
        age = at - row["at"]
        if row["kind"] in ("STUCK", "QUESTION") and age >= stuck and row["resent_at"] is None:
            mark(row, resent_at=at)
            fx.deliver(row["to_node"], message(row))
            if row["to_node"] != "proxy":
                mark(row, escalated_at=at)
                stall(row, "proxy", f"unanswered {round(age / 60)} min, re-sent once")
        elif row["kind"] == "LAND-READY" and age >= land and (row["paged_at"] is None or (
                age >= 2 * land and row["escalated_at"] is None and row["to_node"] != "proxy")):
            if fx.landed(row):
                answer(con, at, [row["id"]], "main")
            elif row["paged_at"] is None:
                mark(row, paged_at=at)
                stall(row, row["to_node"], f"lane not on main {round(age / 60)} min after LAND-READY")
            else:
                mark(row, escalated_at=at)
                stall(row, "proxy", f"lane not on main {round(age / 60)} min after LAND-READY, parent paged")
        elif row["kind"] == "RECYCLE-READY" and age >= recycle and row["restarted_at"] is None:
            mark(row, restarted_at=at)
            error = fx.restart(row["from_node"], row["packet"])
            if error:
                stall(row, "proxy", f"no successor after {round(age / 60)} min and restart failed: {error}")
            else:
                answer(con, at, [row["id"]], "supervisor: agents lead restart")
    for row in con.execute("SELECT * FROM events WHERE delivered_at IS NULL AND answered_at IS NULL ORDER BY id").fetchall():
        error = fx.deliver(row["to_node"], message(row))
        if error:
            mark(row, attempts=row["attempts"] + 1, last_error=error)
        else:
            mark(row, delivered_at=at, attempts=row["attempts"] + 1, last_error="")


def socks():
    return Path(os.environ.get("AGENTS_CC_SOCKS") or f"/run/user/{os.getuid()}/cc-socks")


def socket_of(pid=None, session=None):
    directory = socks()
    if pid and org.alive(pid):
        for p in [pid, *org.descendants(pid)]:
            if (directory / f"{p}.sock").exists():
                return directory / f"{p}.sock"
    if session:
        found = org.address(session.encode())
        if found and Path(found[4:]).parent == directory:
            return Path(found[4:])
    return None


def proxy_socket(data):
    pid = (data.get("proxy") or {}).get("pid")
    if pid:
        return socket_of(pid=pid)
    owned = {p for n in data.get("nodes", []) if n.get("pid") and org.alive(n["pid"])
             for p in [n["pid"], *org.descendants(n["pid"])]}
    candidates = []
    for path in sorted(socks().glob("*.sock")):
        try:
            proc = Path("/proc") / path.stem
            argv = (proc / "cmdline").read_bytes().split(b"\0")
            environ = (proc / "environ").read_bytes().split(b"\0")
        except OSError:
            continue
        if int(path.stem) in owned or any(e.startswith(b"AGENT_ORG_NAME=") for e in environ) or {b"-p", b"--print"} & set(argv):
            continue
        candidates.append(path)
    return candidates[0] if len(candidates) == 1 else None


class Effects:
    def __init__(self, data):
        self.data = data
        self.fetched = set()

    def deliver(self, name, text):
        node = nodes(self.data).get(name)
        if name == "proxy":
            path = proxy_socket(self.data)
            return claude_send(path, text) if path else "no registered or unique proxy socket (agents org set proxy)"
        if node is None:
            return f"{name} is not in agents org"
        if node.get("provider") == "codex":
            if not node.get("session"):
                return "codex node has no thread"
            try:
                r = subprocess.run([os.environ.get("AGENTS_CODEX_SEND") or "codex-lead", "send", text], timeout=60,
                                   env=dict(os.environ, CODEX_LEAD_THREAD=node["session"]), capture_output=True, text=True)
            except (OSError, subprocess.TimeoutExpired) as e:
                return f"codex-lead send: {e}"
            return "" if r.returncode == 0 else f"codex-lead send exit {r.returncode}: {r.stderr.strip()[-200:]}"
        path = socket_of(node.get("pid"), node.get("session"))
        return claude_send(path, text) if path else "no live Claude session socket"

    def landed(self, row):
        main = Path(os.environ.get("AGENTS_CODE_ROOT") or Path.home() / "code") / row["repo"] / "main"
        if not row["repo"] or not row["sha"] or not main.is_dir():
            return False
        if row["repo"] not in self.fetched:
            self.fetched.add(row["repo"])
            subprocess.run(["git", "-C", str(main), "fetch", "-q", "origin"], timeout=120, capture_output=True)
        return subprocess.run(["git", "-C", str(main), "merge-base", "--is-ancestor", row["sha"], "origin/main"],
                              capture_output=True).returncode == 0

    def restart(self, name, packet):
        node = nodes(self.data).get(name)
        if node is None:
            return f"{name} is not in agents org"
        if not os.path.isfile(packet):
            return f"packet {packet} is missing"
        stop(node)
        agents = os.environ.get("AGENTS_BIN") or "agents"
        for argv in ([agents, "org", "set", name, "--brief", packet, "--session", ""], [agents, "lead", "restart", name]):
            try:
                r = subprocess.run(argv, capture_output=True, text=True, timeout=120)
            except (OSError, subprocess.TimeoutExpired) as e:
                return f"{' '.join(argv[1:3])}: {e}"
            if r.returncode:
                return f"{' '.join(argv[1:3])} exit {r.returncode}: {r.stderr.strip()[-200:]}"
        return ""


def claude_send(path, text):
    try:
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as s:
            s.settimeout(3)
            s.connect(str(path))
            s.sendall((json.dumps({"type": "user", "message": {"role": "user", "content": text}}) + "\n").encode())
    except OSError as e:
        return f"socket {path}: {e}"
    return ""


def stop(node):
    tmux = ["tmux", "-L", f"socrates-{node['id']}"]
    if subprocess.run([*tmux, "has-session", "-t", node["id"]], capture_output=True).returncode == 0:
        subprocess.run([*tmux, "kill-session", "-t", node["id"]], capture_output=True)
    pid = node.get("pid")
    if pid and org.alive(pid) and org.owned_pid(pid, node["id"]):
        for target in [*reversed(org.descendants(pid)), pid]:
            try:
                os.kill(target, 15)
            except ProcessLookupError:
                pass
        deadline = time.monotonic() + 10
        while org.alive(pid) and time.monotonic() < deadline:
            time.sleep(0.2)


def supervise(argv):
    if argv not in ([], ["--once"]):
        die(USAGE)
    while True:
        try:
            con = connect()
            data = registry()
            step(con, now(), data, Effects(data))
            con.close()
        except (sqlite3.Error, OSError, ValueError, subprocess.SubprocessError) as e:
            print(f"agents supervise: pass failed: {e}", file=sys.stderr, flush=True)
            if argv:
                sys.exit(1)
        if argv:
            return
        time.sleep(DEFAULTS["event_poll_s"])


def main(argv):
    if not argv or argv[0] in ("-h", "--help"):
        print(USAGE)
        return
    if argv[0] == "supervise":
        return supervise(argv[1:])
    args = argv[1:]
    if args[:1] == ["list"]:
        return listing(args[1:])
    if any(a in ("-h", "--help") for a in args):
        print(USAGE)
        return
    write(args)


if __name__ == "__main__":
    main(sys.argv[1:])
