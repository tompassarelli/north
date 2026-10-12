import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import re


def goal_path():
    return Path(os.environ.get("XDG_STATE_HOME") or Path.home() / ".local/state") / "agents/goal.json"


def main():
    parser = argparse.ArgumentParser(prog="agents goal")
    commands = parser.add_subparsers(dest="command", required=True)
    setter = commands.add_parser("set")
    setter.add_argument("--deadline", required=True)
    setter.add_argument("--projects", required=True)
    setter.add_argument("text", nargs="+")
    commands.add_parser("clear")
    args = parser.parse_args()
    path = goal_path()
    if args.command == "clear":
        path.unlink(missing_ok=True)
        print("agents goal: cleared")
        return
    try:
        deadline = datetime.fromisoformat(args.deadline.replace("Z", "+00:00"))
    except ValueError:
        parser.error("--deadline must be ISO-8601")
    if deadline.tzinfo is None:
        parser.error("--deadline needs a timezone, such as Z")
    projects = [name for name in args.projects.split(",") if name]
    if not projects or any(not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]*", name) for name in projects):
        parser.error("--projects must be comma-separated project names")
    goal = {"text": " ".join(args.text), "projects": projects,
            "deadline": deadline.astimezone(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")}
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(f".{os.getpid()}.tmp")
    temporary.write_text(json.dumps(goal) + "\n")
    temporary.replace(path)
    print(f"agents goal: {goal['text']} by {goal['deadline']} for {', '.join(projects)}")


if __name__ == "__main__":
    main()
