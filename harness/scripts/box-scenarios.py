#!/usr/bin/env python3
"""Sample landed Test-kind commits from a repository and write one policy-replay box scenario per commit."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
import random
import re
import shlex
import subprocess
import sys
import tomllib

DOMAINS = ("north", "firn", "smashcraft")
RUNNERS = ((re.compile(r"\.test\.sh$"), "bash {}"), (re.compile(r"\.test\.py$"), "python3 {}"),
           (re.compile(r"(^|/)test_[^/]*\.py$|_test\.py$"), "python3 {}"), (re.compile(r"\.test\.ts$"), "bun test {}"))
TRAILER = re.compile(r"^[A-Za-z][A-Za-z0-9-]*: ")
ISSUE = re.compile(r"\b[A-Z][A-Z0-9]*-(\d+)\b|(?:\b([\w.-]+))?(?<![/])#(\d+)\b")
BOX = re.compile(r"^\s*[-*] \[[ xX]\] ")


def git(repo: Path, *args: str) -> str:
    return subprocess.run(["git", "-C", str(repo), *args], check=True, capture_output=True, text=True).stdout


def runner(path: str, repo: Path | None = None, sha: str | None = None) -> str | None:
    existing = next((cmd.format(path) for pattern, cmd in RUNNERS if pattern.search(path)), None)
    if existing or repo is None or sha is None:
        return existing
    match = re.fullmatch(r"(crates/[^/]+)/tests/([^/]+)\.rs", path)
    if not match:
        return None
    try:
        manifest = tomllib.loads(git(repo, "show", f"{sha}:{match[1]}/Cargo.toml"))
        source = git(repo, "show", f"{sha}:{path}")
    except (subprocess.CalledProcessError, tomllib.TOMLDecodeError):
        return None
    package = manifest.get("package", {})
    if not package.get("name") or not re.search(r"#\s*\[\s*test\s*\]", source):
        return None
    relative = f"tests/{match[2]}.rs"
    target = next((t for t in manifest.get("test", []) if t.get("path") == relative
                   or ("path" not in t and t.get("name") == match[2])), None)
    if target is not None:
        if target.get("harness", True) is False or target.get("test", True) is False:
            return None
        name = target.get("name")
    elif package.get("autotests", True):
        name = match[2]
    else:
        return None
    if not name:
        return None
    return f"nix develop -c cargo test --locked --offline -p {shlex.quote(package['name'])} --test {shlex.quote(name)}"


def without_trailers(message: str) -> str:
    paragraphs = [p for p in message.strip().split("\n\n") if p.strip()]
    while len(paragraphs) > 1 and all(TRAILER.match(l) for l in paragraphs[-1].splitlines()):
        paragraphs.pop()
    return "\n\n".join(paragraphs).strip()


def github_slug(url: str) -> str | None:
    match = re.search(r"github\.com[:/]([^/]+/[^/]+?)(?:\.git)?/?$", url)
    return match.group(1) if match else None


def issue_brief(slug: str | None, message: str) -> str | None:
    match = ISSUE.search(message)
    if not slug or not match:
        return None
    if match.group(2):
        slug = f"{slug.split('/')[0]}/{match.group(2)}"
    try:
        proc = subprocess.run(["gh", "issue", "view", match.group(1) or match.group(3), "-R", slug, "--json", "title,body"],
                              capture_output=True, text=True, timeout=60)
    except (OSError, subprocess.TimeoutExpired):
        return None
    if proc.returncode != 0:
        return None
    issue = json.loads(proc.stdout)
    body = issue.get("body") or ""
    boxes = [l.strip() for l in body.splitlines() if BOX.match(l)]
    return f"{issue.get('title', '')}\n\n" + ("\n".join(boxes) if boxes else body.strip())


def touched_tests(repo: Path, sha: str, domain: str = "north") -> list[str]:
    paths = git(repo, "show", "--no-renames", "--diff-filter=AM", "--name-only", "--format=", sha).splitlines()
    tests = [p for p in paths if runner(p, repo, sha)]
    if tests:
        tests += [p for p in paths if p not in tests and
                  (p.startswith("tests/") or re.match(r"^crates/[^/]+/tests/", p))]
    return tests


def scenario(repo: Path, url: str, sha: str, tests: list[str], domain: str, cmd: str | None) -> dict:
    message = git(repo, "show", "-s", "--format=%B", sha)
    return {"id": f"box-{domain}-{sha[:12]}", "kind": "box", "repo": url, "commit": sha,
            "brief": issue_brief(github_slug(url), message) or without_trailers(message),
            "detector": {"tests": tests, "cmd": cmd or " && ".join(dict.fromkeys(
                command for p in tests if (command := runner(p, repo, sha))))}, "domain": domain}


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("repo", type=Path)
    ap.add_argument("--since", required=True, help="git --since window, e.g. '30 days ago'")
    ap.add_argument("--domain", required=True, choices=DOMAINS)
    ap.add_argument("--n", type=int, required=True)
    ap.add_argument("--seed", type=int, default=0)
    ap.add_argument("--ref", default="origin/main")
    ap.add_argument("--cmd", help="detector command overriding the one derived from the test file types")
    ap.add_argument("--out", type=Path, help="directory for the scenario files")
    ap.add_argument("--dry-run", action="store_true", help="print the scenarios instead of writing them")
    args = ap.parse_args()
    if not args.dry_run and not args.out:
        ap.error("--out is required without --dry-run")
    repo = args.repo.resolve()
    try:
        url = git(repo, "remote", "get-url", "origin").strip()
    except subprocess.CalledProcessError:
        url = str(repo)
    if Path(url).is_dir():
        url = str(Path(url).resolve())
    shas = git(repo, "log", "--format=%H", "--no-merges", f"--since={args.since}", "--grep=^Test-kind:", args.ref).split()
    built = [(sha, tests) for sha in shas if (tests := touched_tests(repo, sha, args.domain))]
    picked = [scenario(repo, url, sha, tests, args.domain, args.cmd)
              for sha, tests in random.Random(args.seed).sample(built, min(args.n, len(built)))]
    for item in picked:
        if args.dry_run:
            print(json.dumps(item, indent=2))
        else:
            args.out.mkdir(parents=True, exist_ok=True)
            (args.out / f"{item['id']}.json").write_text(json.dumps(item, indent=2) + "\n")
            print(args.out / f"{item['id']}.json")
    if len(picked) < args.n:
        print(f"box-scenarios: {len(picked)} of {args.n} requested: only {len(built)} Test-kind commits with test files", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
