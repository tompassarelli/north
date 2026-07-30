#!/usr/bin/env python3
"""Decide whether one tool call writes into a protected `main` checkout.

BASH COVERAGE

`tool_input.file_path` exists only for Edit/Write/MultiEdit. A Bash call
carries `tool_input.command` instead, so this module parses Bash commands
(git verbs, redirects, in-place/destination-taking commands, interpreter
heredocs) to catch writes that route through the shell rather than a
file_path.

READ vs WRITE

Reads from a primary stay allowed — `git log`, `git status`, `grep`, `cat`. So
does the sanctioned escape route the deny message itself recommends:
`git worktree add` and `git fetch <worktree> <branch>:refs/heads/main`. Only
mutation is refused.

WIP DESTRUCTION

A separate, louder class: `reset --hard`, `stash`, `checkout -- <path>`,
`restore`, `clean -f` against a main checkout throw away uncommitted work that
is the human's, not the lane's. It is checked across EVERY git call in the
command, so a sanctioned verb earlier in the line cannot shield it.

FAIL-OPEN, everywhere. An unparseable command, an unknown shape, any exception:
return None and let the call through.
"""

import json
import os
import re
import shlex
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from launch_critical_paths import protected_project, worktree_advice  # noqa: E402

# git subcommands that change the repository or working tree.
MUTATING_GIT = {
    "commit", "add", "reset", "checkout", "switch", "restore", "merge",
    "rebase", "cherry-pick", "revert", "stash", "apply", "am", "push",
    "clean", "rm", "mv", "gc", "prune", "filter-branch", "update-ref",
}

# Explicitly allowed from a primary — these are how you LEAVE it and how work
# LANDS in it.
SANCTIONED_GIT = {"worktree", "fetch"}

# merge/pull are mutations, but --ff-only cannot dirty the tree or invent a
# commit: it either fast-forwards or refuses. `git fetch <wt> <b>:refs/heads/main`
# — the other landing form — FAILS when main is checked out, which it always is
# under this layout, so without these there is no way to land at all.
FF_ONLY_GIT = {"merge", "pull"}

# Commands whose job is to modify a file in place.
WRITE_COMMANDS = {
    "tee", "truncate", "install", "patch", "dd", "shred",
}

# Commands that write to a destination argument (checked positionally).
COPY_COMMANDS = {"cp", "mv", "rsync", "ln"}

DESTRUCTIVE_COMMANDS = {"rm", "rmdir", "shred"}

INTERPRETERS = {"python", "python3", "perl", "ruby", "node", "bb", "bash", "sh", "zsh"}

# `git stash` subcommands that only report.
STASH_READS = {"list", "show"}

SEGMENT_SPLIT = r'\s*(?:&&|\|\||[;|&\n])\s*'


def _resolve(path, cwd):
    if not path:
        return None
    if not os.path.isabs(path):
        path = os.path.join(cwd, path)
    try:
        return os.path.realpath(path)
    except Exception:
        return None


def _effective_cwd(command, cwd):
    """cwd after any leading `cd <dir>` in the command."""
    m = re.search(r'(?:^|[;&|]|&&)\s*cd\s+("[^"]+"|\'[^\']+\'|[^\s;&|]+)', command)
    if not m:
        return cwd
    target = m.group(1).strip('"\'')
    target = os.path.expanduser(target)
    resolved = _resolve(target, cwd)
    return resolved or cwd


def _strip_heredoc_bodies(command):
    """The command with heredoc BODIES removed, keeping a `<<` marker.

    A heredoc body is DATA, not shell syntax, and must not be scanned for
    redirects or paths. The `<<` itself is preserved because rule 4 still
    needs to know a heredoc was present.
    """
    out, i = [], 0
    for m in re.finditer(r'<<-?\s*[\'"]?(\w+)[\'"]?', command):
        tag = m.group(1)
        out.append(command[i:m.end()])
        end = re.search(r'^\s*%s\s*$' % re.escape(tag),
                        command[m.end():], re.M)
        i = m.end() + (end.end() if end else len(command) - m.end())
    out.append(command[i:])
    return "".join(out)


def _redirect_targets(command):
    """Files the shell would open for writing via > or >>.

    `->` is NOT a redirect (excluded so an arrow inside a quoted string, e.g.
    `echo "a -> b"`, is not treated as one). `>&` (fd duplication, e.g. 2>&1)
    opens no file either, so it is excluded too.
    """
    return [m.group(1).strip('"\'')
            for m in re.finditer(
                r'(?<![-&])>>?\s*(?!&)("[^"]+"|\'[^\']+\'|[^\s;&|<>]+)',
                _strip_heredoc_bodies(command))]


def _tokens(command):
    try:
        return shlex.split(command, comments=False)
    except Exception:
        return command.split()


def _git_invocations(command, cwd):
    """(target, verb, args) for EVERY git call in COMMAND.

    Split per segment first: one sanctioned git call must not vouch for a
    mutating one later in the same line.
    """
    found = []
    for segment in re.split(SEGMENT_SPLIT, _strip_heredoc_bodies(command)):
        tokens = _tokens(segment)
        for i, tok in enumerate(tokens):
            if os.path.basename(tok) != "git":
                continue
            rest = tokens[i + 1:]
            target, verb, j = cwd, None, 0
            while j < len(rest):
                t = rest[j]
                if t == "-C" and j + 1 < len(rest):
                    target = _resolve(os.path.expanduser(rest[j + 1]), cwd) or cwd
                    j += 2
                    continue
                if t.startswith("-"):
                    j += 1
                    continue
                verb = t
                break
            if verb:
                found.append((target, verb, rest[j + 1:]))
    return found


def _short_letters(flags):
    """The letters of clustered short flags: -fd -> {f, d}."""
    return {c for f in flags if re.match(r"^-[A-Za-z]+$", f) for c in f[1:]}


def _git_read_form(verb, args):
    """True when a mutating verb was called in a form that only reports."""
    flags = [a for a in args if a.startswith("-")]
    plain = [a for a in args if not a.startswith("-")]
    if verb == "stash":
        return (plain[0] if plain else "push") in STASH_READS
    if verb == "clean":
        return "--dry-run" in flags or "n" in _short_letters(flags)
    return False


def _git_decision(invocations):
    """(path, verb) for the first mutating git call, else None."""
    for target, verb, args in invocations:
        if verb in SANCTIONED_GIT or _git_read_form(verb, args):
            continue
        if verb in FF_ONLY_GIT:
            # Only the fast-forward form is sanctioned; a bare merge/pull can
            # conflict and leave the checkout dirty, which is the whole problem.
            if "--ff-only" in args:
                continue
            return (target, verb + " (without --ff-only)")
        if verb in MUTATING_GIT:
            return (target, verb)
    return None


def _wip_destroying(verb, args):
    """A short label when this git call discards uncommitted work, else None."""
    flags = [a for a in args if a.startswith("-")]
    plain = [a for a in args if not a.startswith("-")]

    if verb == "reset":
        for f in ("--hard", "--merge", "--keep"):
            if f in flags:
                return "git reset " + f
        return None
    if verb == "stash":
        sub = plain[0] if plain else "push"
        return None if sub in STASH_READS else "git stash " + sub
    if verb == "checkout":
        # Restoring paths overwrites the working tree; switching branches does
        # not, and is caught as an ordinary mutation instead.
        if "--" in args or "." in plain or "-f" in flags or "--force" in flags:
            return "git checkout of working-tree paths"
        return None
    if verb == "restore":
        # --staged alone only unstages; anything else rewrites the working tree.
        if "--staged" in flags and "--worktree" not in flags:
            return None
        return "git restore"
    if verb == "clean":
        if _git_read_form(verb, args):
            return None
        if "--force" in flags or "f" in _short_letters(flags):
            return "git clean -f"
        return None
    return None


def decide(payload):
    """A deny reason string, or None to allow."""
    tool = payload.get("tool_name") or ""
    tool_input = payload.get("tool_input") or {}

    # --- Edit / Write / MultiEdit: the original, unchanged behaviour ---------
    if tool != "Bash":
        path = tool_input.get("file_path")
        hit = protected_project(path)
        if not hit:
            return None
        project, why = hit
        return (f"{path} is inside the PRIMARY checkout of {project}. {why}"
                f"\n\n{worktree_advice(project)}")

    # --- Bash ----------------------------------------------------------------
    command = tool_input.get("command")
    if not isinstance(command, str) or not command:
        return None
    cwd = payload.get("cwd") or os.getcwd()
    eff = _effective_cwd(command, cwd)
    # Tokenise the command WITHOUT heredoc bodies: a `sed -i /path` or
    # `rm /path` appearing inside heredoc data is text being written, not a
    # command being run, and treating it as one denies legitimate work.
    tokens = _tokens(_strip_heredoc_bodies(command))

    def deny(path, project, why, what):
        return (f"This Bash command would {what} inside the PRIMARY checkout of "
                f"{project} ({path}). {why}\n\n"
                f"{worktree_advice(project)}\n"
                f"Reads are fine — it is the write that is refused.")

    invocations = _git_invocations(command, eff)

    # 0. destroying uncommitted work in a main checkout — its own class, because
    #    the loss is the human's and is not recoverable from the ref.
    for target, verb, args in invocations:
        what = _wip_destroying(verb, args)
        if not what:
            continue
        hit = protected_project(target)
        if hit:
            project, why = hit
            return (
                f"`{what}` targets {target}, the MAIN checkout of {project}. "
                f"Uncommitted state in a main checkout is the human's "
                f"work-in-progress; an agent never discards it. {why}\n\n"
                f"Compliant moves:\n"
                f"  git -C {target} status --porcelain   # inspect, do not discard\n"
                f"  # ask the human to clear their own WIP, or work elsewhere:\n"
                f"{worktree_advice(project)}")

    # 1. mutating git, wherever it points
    g = _git_decision(invocations)
    if g:
        target, verb = g
        hit = protected_project(target)
        if hit:
            project, why = hit
            return deny(target, project, why, f"run `git {verb}`")

    # 2. shell redirection into a protected path
    for raw in _redirect_targets(command):
        resolved = _resolve(os.path.expanduser(raw), eff)
        hit = protected_project(resolved)
        if hit:
            project, why = hit
            return deny(resolved, project, why, "write")

    # 3. in-place / destination-taking commands, per SEGMENT.
    #    `cp`/`mv`/`ln` take their destination as the LAST argument — but only
    #    within their own command. Scanning to the end of a compound command
    #    made `ln -s a b; echo "(none = clean)"` treat the echo's text as ln's
    #    destination and deny it. Split on shell separators first.
    for segment in re.split(SEGMENT_SPLIT, _strip_heredoc_bodies(command)):
        found = _scan_write_commands(_tokens(segment), eff, deny)
        if found:
            return found

    # 4. an interpreter fed a heredoc, while cwd is a protected checkout. The
    #    written path lives inside the script and cannot be parsed out, so this
    #    is refused on cwd alone; running it from elsewhere with absolute paths
    #    is unaffected.
    hit = protected_project(eff)
    if hit and "<<" in command:
        for tok in tokens:
            if os.path.basename(tok) in INTERPRETERS:
                return deny(eff, hit[0], hit[1],
                            "run an interpreter script with its working directory")
    return None


def _scan_write_commands(tokens, eff, deny):
    for i, tok in enumerate(tokens):
        base = os.path.basename(tok)
        args = [a for a in tokens[i + 1:] if not a.startswith("-")]
        if base == "sed" and any(a.startswith("-i") for a in tokens[i + 1:]):
            for a in args[1:]:
                hit = protected_project(_resolve(os.path.expanduser(a), eff))
                if hit:
                    return deny(a, hit[0], hit[1], "edit in place")
        elif base in WRITE_COMMANDS or base in DESTRUCTIVE_COMMANDS:
            for a in args:
                hit = protected_project(_resolve(os.path.expanduser(a), eff))
                if hit:
                    return deny(a, hit[0], hit[1], f"run `{base}`")
        elif base in COPY_COMMANDS and args:
            dest = args[-1]
            hit = protected_project(_resolve(os.path.expanduser(dest), eff))
            if hit:
                return deny(dest, hit[0], hit[1], f"run `{base}` into")
    return None


def main():
    try:
        payload = json.load(sys.stdin)
    except Exception:
        return 0
    try:
        reason = decide(payload)
    except Exception:
        return 0  # fail-open
    if reason:
        print(json.dumps({"hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": reason,
        }}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
