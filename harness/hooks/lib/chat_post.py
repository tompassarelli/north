import datetime
import fcntl
import json
import os
from pathlib import Path
import re
import sqlite3
import sys
import time


def read_json(path):
    return json.loads(path.read_text())


def resolve_node(session, state):
    proxy = state / "plato/session-id"
    if proxy.exists() and proxy.read_text().strip() == session:
        return "proxy", None
    org_path = Path(os.environ.get("AGENTS_ORG_FILE", state / "agents/org.json"))
    org = read_json(org_path) if org_path.exists() else {}
    sessions = {node.get("session"): node["id"] for node in org.get("nodes", []) if node.get("session")}
    if session in sessions:
        return sessions[session], None
    pooled = Path(os.environ.get("NORTH_CODEX_POOLED_HOME", Path.home() / ".local/state/north/codex-pooled"))
    database = pooled / "sqlite/state_5.sqlite"
    if not database.exists():
        return None, None
    with sqlite3.connect(database.as_uri() + "?mode=ro", uri=True, timeout=0.03) as db:
        row = db.execute("select agent_path from threads where id = ?", (session,)).fetchone()
        tag = row[0] if row else None
        seen = set()
        while session not in seen:
            seen.add(session)
            edge = db.execute("select parent_thread_id from thread_spawn_edges where child_thread_id = ?", (session,)).fetchone()
            if not edge:
                break
            session = edge[0]
            if session in sessions:
                return sessions[session], tag
            if proxy.exists() and proxy.read_text().strip() == session:
                return "proxy", tag
    return None, None


def spool(state, node, kind, tag, text, source, session):
    if not isinstance(text, str) or not text.strip():
        return
    outbox = state / "socrates-chat/outbox"
    outbox.mkdir(parents=True, exist_ok=True)
    path = outbox / f"{time.time_ns()}-{os.getpid()}.json"
    entry = {"v": 1, "node": node, "kind": kind, "tag": tag, "text": text,
             "file": None, "source": source, "session": session,
             "ts": datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="milliseconds").replace("+00:00", "Z")}
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(entry, ensure_ascii=False))
    temporary.replace(path)


def remote(data, state, node, session):
    inp = data.get("tool_input", {})
    response = data.get("tool_response", {})
    if not isinstance(response, dict) or response.get("status") != 200:
        return
    body = response.get("json", {})
    if isinstance(body, str):
        body = json.loads(body)
    action = inp.get("action")
    store = state / "socrates-chat/remote-runs.json"
    store.parent.mkdir(parents=True, exist_ok=True)
    with store.with_suffix(".lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        runs = read_json(store) if store.exists() else {}
        if action == "run":
            run = body.get("session_id")
            if not run:
                return
            trigger = body.get("trigger", {})
            derived = trigger.get("derived_state", {})
            name = trigger.get("name") or inp.get("trigger_id") or run
            prompt = derived.get("prompt", "").splitlines()
            runs[run] = {"name": name, "seen": []}
            spool(state, node, "notice", name, f"started: {prompt[0] if prompt else ''}, session {run}", "remote-trigger", session)
        elif action in ("get_run_log", "list_runs"):
            records = body.get("data", []) if action == "list_runs" else [body]
            if isinstance(records, dict):
                records = list(records.values())
            for record in records:
                run = record.get("session_id") or record.get("id")
                if not run:
                    continue
                cached = runs.setdefault(run, {"name": record.get("title") or record.get("name") or run, "seen": []})
                summary = response.get("summary", "") if action == "get_run_log" else ""
                status = record.get("status") or record.get("session_status")
                if not status:
                    status = "completed" if re.search(r"(?:^|\n)(?:\[[^\]\n]+\]\s+)?result:", summary, re.I) else "running"
                branch = record.get("branch") or record.get("branch_name")
                if not branch:
                    match = re.search(r"\bclaude/[A-Za-z0-9._/-]+", summary)
                    branch = match.group(0) if match else None
                if status in cached["seen"]:
                    continue
                text = f"status: {status}" + (f"; branch {branch}" if branch else "")
                if summary:
                    text += "\n" + summary
                spool(state, node, "result", cached["name"], text, "remote-trigger", session)
                cached["seen"].append(status)
        else:
            return
        temporary = store.with_suffix(".tmp")
        temporary.write_text(json.dumps(runs))
        temporary.replace(store)


def main():
    data = json.loads(sys.stdin.buffer.read(4 * 1024 * 1024))
    session = data.get("session_id")
    if not session:
        return
    state = Path(os.environ.get("XDG_STATE_HOME", Path.home() / ".local/state"))
    node, child_tag = resolve_node(session, state)
    if node is None:
        return
    event = data.get("hook_event_name")
    text = data.get("last_assistant_message", "")
    if event == "PostToolUse" and data.get("tool_name") == "RemoteTrigger":
        remote(data, state, node, session)
    elif event == "SubagentStop":
        transcript = Path(data.get("transcript_path", ""))
        agent_transcript = Path(data.get("agent_transcript_path", transcript.with_suffix("") / "subagents" / f"agent-{data.get('agent_id')}.jsonl"))
        meta = agent_transcript.with_suffix(".meta.json")
        tag = read_json(meta).get("description") if meta.exists() else data.get("agent_type") or data.get("agent_id")
        spool(state, node, "notice", tag, text, "claude-subagent", session)
    elif event == "Stop":
        codex = "turn_id" in data
        spool(state, node, "notice" if child_tag else "reply", child_tag, text,
              "codex-stop" if codex else "claude-stop", session)


if __name__ == "__main__":
    try:
        main()
    except Exception:
        pass
