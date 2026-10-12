import argparse
from concurrent.futures import ThreadPoolExecutor
from datetime import date, datetime, timedelta
import json
import os
from pathlib import Path
import re
import subprocess
import sys

VOLATILE = re.compile(r"(?<![\w#.-])\d+(?:\.\d+)?")
SWEEP_FLAGS = ("STALLED", "OVERTIME", "HANDOFF", "PARKED", "CAPACITY")
STALE_IDLE_MINUTES = 300
SLOW_SOURCES = ("flow", "ledger")


def norm(text):
    return VOLATILE.sub("N", " ".join(text.split()))


def reduce_unowned(out):
    rows, counts = [], ""
    for line in out.splitlines():
        if line.startswith("priority:now "):
            counts = line.strip()
            continue
        cols = line.split("\t")
        if len(cols) >= 5 and cols[1] == "priority:now" and cols[2] == "UNOWNED":
            rows.append((f"unowned {cols[0]}", line))
    return rows, counts


def reduce_sweep(out):
    rows = []
    for line in out.splitlines():
        cols = line.split()
        if not cols:
            continue
        if cols[0] in ("WAIT", "STALE"):
            rows.append((norm(line), line))
        elif cols[0] == "PARKED" and len(cols) > 1:
            rows.append((f"PARKED {cols[1].rstrip(':')}", line))
        else:
            idle = next((int(c[5:-1]) for c in cols if re.fullmatch(r"idle=\d+m", c)), 0)
            for flag in (f for f in cols[-1].split(",") if f in SWEEP_FLAGS):
                if flag == "STALLED" and idle > STALE_IDLE_MINUTES:
                    continue
                rows.append((f"{flag} {cols[0]}", line))
    return rows


def reduce_capacity(out):
    rows = []
    for line in out.splitlines():
        try:
            report = json.loads(line)
        except ValueError:
            continue
        for incident in report.get("incidents", []):
            match = re.match(r"- (\S+) ([^:\s]+)", incident)
            if match:
                rows.append((f"incident {match.group(1)} {match.group(2)}", incident))
        for flag in report.get("flags", []):
            if not re.fullmatch(r"\d+ incident\(s\) in window", flag):
                rows.append((f"capacity {norm(flag)}", flag))
    return rows


def reduce_flow(out):
    rows = []
    for line in out.splitlines():
        match = re.match(r"([\w.-]+): pending .*?, stall (\w+)", line)
        if match and match.group(2) != "healthy":
            rows.append((f"flow {match.group(1)} stall {match.group(2)}", line))
        dead = re.match(r"([\w.-]+): pending \S+, dead (\d+) \(oldest (\d+)m\)", line)
        if dead and int(dead.group(3)) > 30:
            rows.append((f"flow {dead.group(1)} dead lanes", line))
        repo = re.match(r"([\w.-]+): pending ", line)
        pace = re.search(r", closed ([\d.]+)/h needed ([\d.]+)/h", line)
        if repo and pace and float(pace.group(1)) < float(pace.group(2)):
            rows.append((f"flow {repo.group(1)} goal closures behind", line))
        close = re.search(r", land-to-close (\d+)m", line)
        if repo and close and int(close.group(1)) > 30:
            rows.append((f"flow {repo.group(1)} goal land-to-close slow", line))
        ratio = re.search(r", land ratio ([\d.]+) over 3h", line)
        if repo and ratio and float(ratio.group(1)) < 1.0:
            rows.append((f"flow {repo.group(1)} goal land ratio low", line))
        unowned = re.search(r", failed unowned (\d+)", line)
        if repo and unowned and int(unowned.group(1)) > 0:
            rows.append((f"flow {repo.group(1)} goal failed unowned", line))
        stale = re.match(r"agents flow: no transcript .*: (.+)", line)
        if stale:
            rows.extend((f"flow untranscribed {name}", line) for name in stale.group(1).split(", "))
        if line.startswith("agents flow: usage cache stale"):
            rows.append(("flow north usage-web-read stale", line))
        nameless = re.match(r"agents flow: not reachable by name .*: (.+)", line)
        if nameless:
            rows.extend((f"flow unnamed {name}", line) for name in nameless.group(1).split(", "))
    return rows


def reduce_ledger(out):
    lines = out.splitlines()
    start = next((i for i, line in enumerate(lines) if line.startswith("category ")), None)
    if start is None:
        return []
    lines = lines[start:]
    header = lines[0]
    starts = [m.start() for m in re.finditer(r"\S+", header)]
    names = header.split()
    rows = []
    for line in lines[1:]:
        if not line.strip() or line[:1] == " ":
            break
        cell = {name: line[a:b].strip() for name, a, b in zip(names, starts, starts[1:] + [None])}
        try:
            n = int(cell["n"])
        except (KeyError, ValueError):
            continue
        if n < 5:
            continue
        cat = f"{cell['category']} {cell['tier']} {cell['arm']}"
        x = cell.get("x_eta_med", "-")
        if x != "-" and not 0.5 <= float(x) <= 2:
            rows.append((f"ledger {cat} eta-miscalibrated", line))
        land = re.fullmatch(r"(\d+)/(\d+)", cell.get("no_landing", ""))
        if land and int(land.group(2)) and int(land.group(1)) / int(land.group(2)) > 0.3 and cell["category"] != "research":
            rows.append((f"ledger {cat} no-landing", line))
    return rows


def lead_of(nodes, name):
    by_id = {n["id"]: n for n in nodes}
    node, seen = by_id.get(name), set()
    while node and node.get("role") != "lead" and node["id"] not in seen:
        seen.add(node["id"])
        node = by_id.get(node.get("parent"))
    return node["id"] if node else "proxy"


def repo_leads(nodes, repo):
    return {n["id"] for n in nodes if n.get("role") == "lead" and n.get("state", "active") == "active"
            and (n.get("domain") == repo or repo in (n.get("repos") or []))}


def owners(source, key, nodes):
    if source == "capacity":
        org = re.search(r" org=(\S+)", key)
        return {lead_of(nodes, org.group(1))} if org else {"proxy"}
    if source == "ledger":
        return {"proxy"}
    if source == "unowned":
        return repo_leads(nodes, key.split()[1].split("#")[0]) | {"proxy"}
    if source == "sweep":
        repo = re.match(r"WAIT .*?[\w.-]+/([\w.-]+) has |STALE ([\w.-]+):", key)
        return (repo_leads(nodes, repo.group(1) or repo.group(2)) or {"proxy"}) if repo else None
    if source != "flow":
        return None
    named = re.match(r"flow (?:untranscribed|unnamed) (\S+)", key)
    if named:
        return {lead_of(nodes, named.group(1)), "proxy"}
    return repo_leads(nodes, key.split()[1]) or {"proxy"}


def report_files(node):
    status = node.get("status_file") or ""
    if not status:
        return []
    return [status, status.removesuffix("-status.md").removesuffix(".md") + "-peer.md"]


def done_children(parent, nodes, files):
    done = []
    for node in nodes:
        if node.get("parent") != parent or node.get("state", "active") != "active":
            continue
        seen = [files[p] for p in report_files(node) if p in files]
        if not seen:
            continue
        mtime, text = max(seen, key=lambda f: f[0])
        last = next((line for line in reversed(text.splitlines()) if line.strip()), "")
        started = datetime.strptime(node["started"], "%Y-%m-%dT%H:%M:%S%z").timestamp() if node.get("started") else 0
        if re.match(r"\W*DONE\b", last) and mtime > started:
            done.append(node["id"])
    return done


def finish_done(session, nodes):
    me = next((n["id"] for n in nodes if n.get("session") == session), None) or os.environ.get("AGENT_ORG_NAME") or "proxy"
    files = {}
    for path in {p for n in nodes if n.get("parent") == me for p in report_files(n)}:
        try:
            files[path] = (os.stat(path).st_mtime, Path(path).read_text(errors="replace"))
        except OSError:
            pass
    finished = []
    for name in done_children(me, nodes, files):
        if subprocess.run(["agents", "org", "finish", name], capture_output=True).returncode == 0:
            finished.append(f"FINISHED {name}")
    return finished


def viewer_of(nodes, session):
    node = next((n for n in nodes if n.get("session") == session), None)
    return lead_of(nodes, node["id"]) if node else "proxy"


REDUCERS = {"sweep": reduce_sweep, "capacity": reduce_capacity, "flow": reduce_flow, "ledger": reduce_ledger}


def decide(outputs, prev_keys, tick, viewer=None, nodes=()):
    rows, counts = reduce_unowned(outputs.get("unowned", ""))
    current = [("unowned", key, row) for key, row in rows]
    for source, reducer in REDUCERS.items():
        if source in outputs:
            current += [(source, key, row) for key, row in reducer(outputs[source])]
    current += [(source, f"failed {source}", out.strip()) for source, out in outputs.items() if out.startswith("tick-precheck: ")]
    if viewer is not None:
        current = [row for row in current if (o := owners(row[0], row[1], nodes)) is None or viewer in o]
    keys = {key for _, key, _ in current}
    keys |= {k for k in prev_keys if k.split(" ", 1)[0] in SLOW_SOURCES and not any(s in outputs for s in SLOW_SOURCES)}
    seen, new = set(prev_keys), []
    for source, key, row in current:
        if key not in seen:
            seen.add(key)
            new.append(f"{source}: {row}")
    summary = f"{counts or 'priority:now unowned=?'} flags={len(keys)}"
    if new:
        return 10, new, sorted(keys)
    return 0, [f"quiet tick={tick}: {summary}"], sorted(keys)


def last_tick(status):
    try:
        text = Path(status).read_text()
    except OSError:
        return -1
    found = re.findall(r"\btick=(\d+)", text)
    return int(found[-1]) if found else -1


def commands(session, tick):
    cmds = {
        "unowned": ["threads", "unowned"],
        "sweep": ["worker-sweep", "--session", session],
        "capacity": ["capacity-watchdog", "report", "--minutes", "5", "--minutes", "30"],
    }
    if tick % 3 == 0:
        cmds["flow"] = ["agents", "flow"]
        cmds["ledger"] = ["worker-ledger", "--summary", "--since", (date.today() - timedelta(days=7)).isoformat()]
    return cmds


def run(cmd):
    try:
        done = subprocess.run(cmd, capture_output=True, text=True, timeout=110)
        return done.stdout + done.stderr
    except (OSError, subprocess.TimeoutExpired) as e:
        return f"tick-precheck: {cmd[0]} failed: {e}\n"


def main(argv):
    parser = argparse.ArgumentParser(prog="agents tick-precheck")
    parser.add_argument("--session", required=True)
    parser.add_argument("--status", required=True)
    args = parser.parse_args(argv)
    state_dir = Path(os.environ.get("XDG_STATE_HOME") or Path.home() / ".local/state") / "agents/tick-precheck"
    state_dir.mkdir(parents=True, exist_ok=True)
    state_path = state_dir / f"{args.session}.json"
    try:
        prev = json.loads(state_path.read_text()).get("keys", [])
    except (OSError, ValueError):
        prev = []
    tick = last_tick(args.status) + 1
    cmds = commands(args.session, tick)
    with ThreadPoolExecutor(len(cmds)) as pool:
        outputs = dict(zip(cmds, pool.map(run, cmds.values())))
    (state_dir / f"{args.session}-last.txt").write_text(
        "".join(f"## {name} tick={tick}: {' '.join(cmds[name])}\n{out}\n" for name, out in outputs.items()))
    try:
        nodes = json.loads(Path(os.environ.get("AGENTS_ORG_FILE") or state_dir.parent / "org.json").read_text()).get("nodes", [])
    except (OSError, ValueError):
        nodes = []
    code, lines, keys = decide(outputs, prev, tick, viewer_of(nodes, args.session), nodes)
    finished = finish_done(args.session, nodes)
    if finished:
        code, lines = 10, finished + (lines if code else [])
    state_path.write_text(json.dumps({"tick": tick, "keys": keys}, indent=1) + "\n")
    print("\n".join(lines))
    if code == 0:
        with open(args.status, "a") as f:
            f.write(lines[0] + "\n")
    return code


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
