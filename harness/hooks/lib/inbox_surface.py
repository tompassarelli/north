import fcntl
import hashlib
import json
import os
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "lib"))
import events
import org

LIMIT = 16384


def surface(data):
    event = data.get("hook_event_name")
    session = data.get("session_id")
    name = os.environ.get("AGENT_ORG_NAME")
    if event not in ("PostToolUse", "UserPromptSubmit") or not session or not name or "agent_id" in data:
        return
    registry = json.loads(Path(org.org_file()).read_text())
    node = next((n for n in registry.get("nodes", []) if n["id"] == name), None)
    if name == "proxy":
        node = dict(registry.get("proxy", {}), id="proxy")
    if node is None or node.get("session") != session:
        return
    inbox = events.inbox_path(node)
    state = Path(os.environ["XDG_RUNTIME_DIR"]) / "north-inbox-surface"
    state.mkdir(mode=0o700, exist_ok=True)
    key = hashlib.sha256(json.dumps([session, name, str(inbox)]).encode()).hexdigest()
    with (state / key).open("a+") as marker, inbox.open("rb") as source:
        fcntl.flock(marker, fcntl.LOCK_EX | fcntl.LOCK_NB)
        marker.seek(0)
        previous = marker.read()
        end = os.fstat(source.fileno()).st_size
        offset = int(previous) if previous else end
        offset = 0 if offset > end else offset
        source.seek(offset)
        raw = source.read(min(end - offset, LIMIT))
        cut = end - offset - len(raw)
        context = raw.decode("utf-8", errors="replace").lstrip("\n")
        if cut:
            context += f"\n[inbox: {cut} bytes cut]"
        marker.seek(0)
        marker.truncate()
        marker.write(str(end))
        marker.flush()
        if context:
            return {"hookSpecificOutput": {"hookEventName": event, "additionalContext": context}}


if __name__ == "__main__":
    try:
        output = surface(json.load(sys.stdin))
        if output:
            print(json.dumps(output))
    except Exception:
        pass
