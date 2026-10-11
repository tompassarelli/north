import json
import os
import re
import shlex
import subprocess
import sys
from pathlib import Path

from org import org_file


ALIASES = {"nixos-config": "firn"}


def archive_mentions(message, repo, number):
    for line in message.splitlines():
        match = re.search(r"\bRefs\s*:?\s+(.+)", line, re.I)
        if match:
            refs = re.findall(r"(?<![\w/.-])(?:tompassarelli/)?([\w.-]+)#(\d+)(?!\d)", match[1])
            if any(ALIASES.get(name, name) == repo and int(n) == number for name, n in refs):
                return True
    return False


def checkout(repo, cwd=None):
    name = "nixos-config" if repo == "firn" else repo
    container = Path.home() / "code" / name
    current = Path(cwd or os.getcwd()).resolve()
    return current if current.is_relative_to(container.resolve()) else container / "main"


def git(path, *args):
    try:
        result = subprocess.run(["git", "-c", "maintenance.auto=false", "-c", "gc.auto=0", "-C", str(path), *args],
                                capture_output=True, text=True, timeout=15,
                                env={**os.environ, "GIT_TERMINAL_PROMPT": "0", "GIT_OPTIONAL_LOCKS": "0"})
    except (OSError, subprocess.TimeoutExpired):
        return None
    return result.stdout if result.returncode == 0 else None


def archives(repo, number, cwd=None):
    path = checkout(repo, cwd)
    if not path.is_dir():
        return []
    listing = git(path, "ls-remote", "--tags", "origin", "refs/tags/archive/*")
    if listing is None:
        return []
    tags = {name: sha for sha, name in (line.split() for line in listing.splitlines()) if not name.endswith("^{}")}
    missing = [f"{name}:{name}" for name, sha in tags.items()
               if (git(path, "rev-parse", "--verify", name) or "").strip() != sha]
    if missing and git(path, "fetch", "--no-tags", "--no-write-fetch-head", "origin", *missing) is None:
        return []
    return [name.removeprefix("refs/tags/") for name, sha in sorted(tags.items())
            if archive_mentions(git(path, "log", "--format=%B", sha, "--") or "", repo, number)]


def print_archives(repo, number, cwd=None):
    for tag in archives(repo, number, cwd):
        print(f"  archived: {tag}")
        print(f"    restore: git push origin {shlex.quote(tag + ':refs/heads/' + tag.removeprefix('archive/'))}")


def session_item(payload):
    try:
        event = json.loads(payload or "{}")
        nodes = json.loads(Path(org_file()).read_text()).get("nodes", [])
    except (OSError, ValueError):
        return None
    session = event.get("session_id")
    node = next((n for n in nodes if session and n.get("session") == session), None)
    if node is None:
        node = next((n for n in nodes if n.get("id") == os.environ.get("AGENT_ORG_NAME")
                     and (not session or not n.get("session"))), None)
    if not node or not node.get("brief"):
        return None
    try:
        brief = Path(node["brief"]).read_text()
    except OSError:
        return None
    match = re.search(r"\bItem:\s*(?:tompassarelli/)?([\w.-]+)#(\d+)", brief)
    return (ALIASES.get(match[1], match[1]), int(match[2]), event.get("cwd")) if match else None


if __name__ == "__main__":
    item = session_item(sys.stdin.read())
    if item:
        print_archives(*item)
