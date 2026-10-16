import json
import os
import re
import shlex
import subprocess
import sys
from pathlib import Path

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
    listed = git(path, "for-each-ref", "--format=%(refname) %(objectname)", "refs/tags/archive/")
    local = dict(line.split(" ", 1) for line in listed.splitlines()) if listed is not None else None
    missing = [f"{name}:{name}" for name, sha in tags.items()
               if (local.get(name) if local is not None else (git(path, "rev-parse", "--verify", name) or "").strip()) != sha]
    if missing and git(path, "fetch", "--no-tags", "--no-write-fetch-head", "origin", *missing) is None:
        return []
    shas = sorted(set(tags.values()))
    log = git(path, "log", "--format=%H%x00%B%x1e", *shas, "--") if shas else ""
    if log is not None:
        hits = [entry.lstrip("\n").split("\0", 1)[0] for entry in log.split("\x1e")
                if "\0" in entry and archive_mentions(entry.split("\0", 1)[1], repo, number)]
        holders = git(path, "for-each-ref", "--format=%(objectname)", *(f"--contains={c}" for c in hits),
                      "refs/tags/archive/") if hits else ""
        if holders is not None:
            return [name.removeprefix("refs/tags/") for name, sha in sorted(tags.items()) if sha in holders.split()]
    return [name.removeprefix("refs/tags/") for name, sha in sorted(tags.items())
            if archive_mentions(git(path, "log", "--format=%B", sha, "--") or "", repo, number)]


def print_archives(repo, number, cwd=None):
    for tag in archives(repo, number, cwd):
        print(f"  archived: {tag}")
        print(f"    restore: git push origin {shlex.quote(tag + ':refs/heads/' + tag.removeprefix('archive/'))}")


def session_item(payload):
    try:
        event = json.loads(payload or "{}")
        nodes = json.loads(subprocess.run([os.environ.get("NORTH_BIN", "north"), "fleet", "list", "--json"],
                                          capture_output=True, text=True, timeout=5, check=True).stdout)
    except (OSError, ValueError, subprocess.SubprocessError):
        return None
    session = event.get("session_id")
    node = next((n for n in nodes if session and n.get("conversation") == session and n.get("status") != "exited"), None)
    match = re.fullmatch(r"(?:tompassarelli/)?([\w.-]+)#(\d+)", node.get("task_ref") or "") if node else None
    return (ALIASES.get(match[1], match[1]), int(match[2]), event.get("cwd")) if match else None


if __name__ == "__main__":
    item = session_item(sys.stdin.read())
    if item:
        print_archives(*item)
