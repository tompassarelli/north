#!/usr/bin/env python3
"""Decide whether one tool call writes into a protected checkout.

TWO PROTECTED SHAPES, TWO REMEDIES

A `main/` checkout is protected because it is the clean product and any dirt in
it is the human's; the remedy is a lane under `<container>/worktrees/`. A
`pins/<name>/` checkout is protected because something OUTSIDE the repository
consumes it at that exact path and revision; the remedy is the `.pin` manifest
and the human, never `worktree add` — sending an agent to cut a lane from a pin
sends it to break the thing being protected. `protected_project` returns the
kind so both nouns and both remedies stay correct at every deny site.

BASH COVERAGE

`tool_input.file_path` exists only for Edit/Write/MultiEdit. A Bash call
carries `tool_input.command` instead, so this module parses Bash commands
(git verbs, redirects, in-place/destination-taking commands, interpreter
heredocs) to catch writes that route through the shell rather than a
file_path.

APPLY_PATCH

apply_patch carries Add/Update/Delete File targets and Move to destinations
inside its patch envelope. Both direct tool calls and Bash string/argv entrances
are parsed.

READ vs WRITE

Reads from a primary stay allowed — `git log`, `git status`, `grep`, `cat`. So
does the sanctioned escape route the deny message itself recommends:
`git worktree add` (into `<container>/worktrees/`) and
`git fetch LANE BRANCH:refs/heads/main`. A pin's one sanctioned mutation —
`git -C <pin> checkout REF`, re-pointing it — is allowed for the same reason.
Only unsanctioned mutation is refused.

WIP DESTRUCTION

A separate, louder class: `reset --hard`, `stash`, `checkout -- <path>`,
`restore`, `clean -f` against a main checkout throw away uncommitted work that
is the human's, not the lane's. It is checked across EVERY git call in the
command, so a sanctioned verb earlier in the line cannot shield it.

SANCTIONED TOOLS

`wt-rescue` is the remediation this guard's deny message recommends, and it
performs internally the very operations denied raw. Its own command segment is
excised before any rule runs; every other segment on the line is still scanned.

FAIL-OPEN for unparseable commands, unknown shapes, and internal exceptions. A
recognized apply_patch call whose envelope cannot be parsed is denied: it is a
write, and a write whose targets cannot be read cannot be checked.
"""

import json
import os
import re
import shlex
import subprocess
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from launch_critical_paths import (  # noqa: E402
    hit_advice, hit_noun, is_pin, protected_project, worktree_advice)

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
# commit: it either fast-forwards or refuses. `git fetch LANE BRANCH:refs/heads/main`
# — the other landing form — FAILS when main is checked out, which it always is
# under this layout, so without these there is no way to land at all.
FF_ONLY_GIT = {"merge", "pull"}

# A pin's ONE sanctioned mutation: re-pointing it at another revision. Narrow on
# purpose — a bare `checkout REF` / `switch --detach REF` moves HEAD and nothing
# else. Path arguments (`checkout -- x`, `checkout .`), branch creation, and
# force all rewrite the working tree a consumer is reading, so none of them ride
# in on this carve-out.
PIN_REPOINT_GIT = {"checkout", "switch"}
PIN_REPOINT_FLAGS = {"-q", "--quiet", "--detach"}

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

# Tools whose whole job is a sanctioned remediation of a protected checkout.
# Denying one leaves the deny message recommending a move the guard forbids.
SANCTIONED_TOOLS = {"wt-rescue"}

# `bun install` is not install(1). A package manager's subcommand collides with
# a real write command's name, and its arguments are package names, not paths.
PACKAGE_MANAGERS = {
    "bun", "bunx", "npm", "npx", "pnpm", "pnpx", "yarn", "deno",
    "cargo", "pip", "pip3", "uv", "poetry", "gem", "go", "nix",
}

# Header forms are the complete grammar the Codex binary accepts:
# Add File / Update File / Delete File, plus Move to inside an update hunk.
PATCH_BEGIN = "*** Begin Patch"
PATCH_FILE_HEADER = re.compile(r"^\*\*\* (?:Add|Update|Delete) File:\s+(.+?)\s*$", re.M)
PATCH_MOVE_HEADER = re.compile(r"^\*\*\* Move to:\s+(.+?)\s*$", re.M)
PATCH_ENVELOPE = re.compile(r"\*\*\* Begin Patch.*?(?:\*\*\* End Patch|\Z)", re.S)

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


def _tracks_nothing(path):
    """True when git tracks no file at or under `path`.

    Removing such a path cannot change tracked state, so the guard has nothing
    to protect there — it is not part of the tree. Fails closed: any git error
    reports False and the caller denies as before.
    """
    if not path:
        return False
    try:
        parent = path if os.path.isdir(path) else os.path.dirname(path)
        result = subprocess.run(
            ["git", "-C", parent, "ls-files", "--error-unmatch", "--", path],
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=5)
        if result.returncode == 0:
            return False
        listed = subprocess.run(
            ["git", "-C", parent, "ls-files", "--", path],
            capture_output=True, text=True, timeout=5)
        return listed.returncode == 0 and not listed.stdout.strip()
    except Exception:
        return False


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


def _redirect_targets(text):
    """Files the shell would open for writing via > or >>.

    TEXT is already heredoc-stripped: a redirect inside a heredoc BODY is data.
    `->` is NOT a redirect (excluded so an arrow inside a quoted string, e.g.
    `echo "a -> b"`, is not treated as one). `>&` (fd duplication, e.g. 2>&1)
    opens no file either, so it is excluded too.
    """
    return [m.group(1).strip('"\'')
            for m in re.finditer(
                r'(?<![-&])>>?\s*(?!&)("[^"]+"|\'[^\']+\'|[^\s;&|<>]+)', text)]


def _tokens(command):
    try:
        return shlex.split(command, comments=False)
    except Exception:
        return command.split()


# `rm x 2>/dev/null` has one target, not two: a redirection is shell syntax,
# not an argument. shlex keeps it as a token, so it has to be dropped here.
_REDIRECT_TOKEN = re.compile(r"^\d*(?:>>|>|<)")


def _redirection(token):
    return bool(_REDIRECT_TOKEN.match(token))


def _leads_with(segment, names):
    """True when SEGMENT invokes one of NAMES as its own command."""
    for tok in _tokens(segment):
        if re.match(r"^[A-Za-z_][A-Za-z_0-9]*=", tok) or tok in ("env", "exec", "command"):
            continue
        return os.path.basename(tok) in names
    return False


def _leads_with_sanctioned_tool(segment):
    """True when SEGMENT invokes a sanctioned tool as its own command."""
    return _leads_with(segment, SANCTIONED_TOOLS)


def _excise_sanctioned(text):
    """TEXT with sanctioned-tool segments dropped.

    Per SEGMENT, never per command: `wt-rescue x && git -C main reset --hard`
    still gets its second half scanned. Separators are kept verbatim — the
    segment split cuts through `2>&1`, so rejoining without them would forge a
    redirect out of an fd duplication.
    """
    parts = re.split("(" + SEGMENT_SPLIT + ")", text)
    return "".join(p for i, p in enumerate(parts)
                   if i % 2 or not _leads_with_sanctioned_tool(p))


def _patch_targets(envelope):
    """Raw file targets named by one apply_patch envelope."""
    return (PATCH_FILE_HEADER.findall(envelope)
            + PATCH_MOVE_HEADER.findall(envelope))


def _find_envelope(value):
    """The first nested string containing an apply_patch envelope."""
    if isinstance(value, str):
        return value if PATCH_BEGIN in value else None
    if isinstance(value, dict):
        for child in value.values():
            found = _find_envelope(child)
            if found is not None:
                return found
    return None


def _invokes_apply_patch(stripped_text):
    """True when a shell segment invokes apply_patch as its command."""
    return any(_leads_with(segment, {"apply_patch"})
               for segment in re.split(SEGMENT_SPLIT, stripped_text))


def _apply_patch_fail_closed():
    return (
        "This apply_patch call is denied fail-closed: a patch whose targets "
        "cannot be read cannot be checked. Re-issue it with well-formed "
        "`*** Add/Update/Delete File:` headers. Deliberate bypass: "
        "`north config guards off`.")


def _apply_patch_target_decision(targets, cwd):
    for target in targets:
        path = _resolve(os.path.expanduser(target), cwd)
        hit = protected_project(path)
        if hit:
            project, why, kind = hit
            return (f"This apply_patch envelope would write {path} inside "
                    f"{hit_noun(project, kind)}. {why}\n\n"
                    f"{hit_advice(project, kind)}")
    return None


def _apply_patch_tool_decision(tool_input, payload):
    """Decision for a direct apply_patch tool call."""
    cwd = payload.get("cwd") or os.getcwd()
    if isinstance(tool_input, dict):
        explicit = [tool_input.get(key) for key in ("file_path", "path")]
        verdict = _apply_patch_target_decision(
            [value for value in explicit if isinstance(value, str)], cwd)
        if verdict:
            return verdict
        selected_cwd = tool_input.get("workdir") or tool_input.get("cwd")
        patch_cwd = selected_cwd if isinstance(selected_cwd, str) else cwd
    else:
        patch_cwd = cwd

    envelope = _find_envelope(tool_input)
    if envelope is None:
        return _apply_patch_fail_closed()
    targets = _patch_targets(envelope)
    if not targets:
        return _apply_patch_fail_closed()
    return _apply_patch_target_decision(targets, patch_cwd)


def _apply_patch_command_decision(command, cwd, stripped=None):
    """Decision for apply_patch invoked through a shell string or argv."""
    if isinstance(command, str):
        stripped = stripped if stripped is not None else _strip_heredoc_bodies(command)
        invoked = _invokes_apply_patch(stripped)
        envelopes = PATCH_ENVELOPE.findall(command)
    elif isinstance(command, list):
        invoked = bool(command and os.path.basename(command[0]) == "apply_patch")
        if (not invoked and command
                and os.path.basename(command[0]) in {"bash", "sh", "zsh"}):
            for i, flag in enumerate(command[1:-1], start=1):
                if flag.startswith("-") and "c" in flag[1:]:
                    inner = command[i + 1]
                    invoked = _invokes_apply_patch(_strip_heredoc_bodies(inner))
                    break
        envelopes = PATCH_ENVELOPE.findall("\n".join(command))
    else:
        return None

    if not invoked:
        return None
    if not envelopes:
        return _apply_patch_fail_closed()
    for envelope in envelopes:
        targets = _patch_targets(envelope)
        if not targets:
            return _apply_patch_fail_closed()
        verdict = _apply_patch_target_decision(targets, cwd)
        if verdict:
            return verdict
    return None


def _blank_quoted(text):
    """Blank single/double-quoted spans so quoted PROSE naming a git command
    (a manifest line, a commit message, an echo) is never scanned as a live
    invocation. A real call's `git`, flags, and verb sit outside quotes; the
    known cost is a quoted -C argument losing its target, which degrades to
    cwd — a narrower miss than the prose false-positive it removes."""
    out, i, n, q = [], 0, len(text), None
    while i < n:
        c = text[i]
        if q:
            if q == '"' and c == "\\" and i + 1 < n:
                out.append("  ")
                i += 2
                continue
            if c == q:
                q = None
                out.append(" ")
            else:
                out.append("\n" if c == "\n" else " ")
            i += 1
            continue
        if c in ("'", '"'):
            q = c
            out.append(" ")
            i += 1
            continue
        out.append(c)
        i += 1
    return "".join(out)


def _git_invocations(text, cwd):
    """(target, verb, args) for EVERY git call in TEXT.

    Split per segment first: one sanctioned git call must not vouch for a
    mutating one later in the same line.
    """
    found = []
    for segment in re.split(SEGMENT_SPLIT, _blank_quoted(text)):
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


def _pin_repoint(target, verb, args):
    """True when this git call is the one mutation a pin sanctions."""
    if verb not in PIN_REPOINT_GIT:
        return False
    flags = [a for a in args if a.startswith("-")]
    plain = [a for a in args if not a.startswith("-")]
    if "--" in args or "." in plain or len(plain) != 1:
        return False
    if any(flag not in PIN_REPOINT_FLAGS for flag in flags):
        return False
    hit = protected_project(target)
    return bool(hit) and is_pin(hit[2])


def _git_decision(invocations):
    """(path, verb) for the first mutating git call, else None."""
    for target, verb, args in invocations:
        if verb in SANCTIONED_GIT or _git_read_form(verb, args):
            continue
        if _pin_repoint(target, verb, args):
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

    # --- apply_patch ----------------------------------------------------------
    if tool.endswith("apply_patch"):
        return _apply_patch_tool_decision(tool_input, payload)

    # --- Edit / Write / MultiEdit: the original, unchanged behaviour ---------
    if tool != "Bash":
        path = tool_input.get("file_path")
        hit = protected_project(path)
        if not hit:
            return None
        project, why, kind = hit
        return (f"{path} is inside {hit_noun(project, kind)}. {why}"
                f"\n\n{hit_advice(project, kind)}")

    # --- Bash ----------------------------------------------------------------
    command = tool_input.get("command")
    if isinstance(command, list) and all(isinstance(x, str) for x in command):
        verdict = _apply_patch_command_decision(
            command, payload.get("cwd") or os.getcwd())
        if verdict:
            return verdict
        return None
    if not isinstance(command, str) or not command:
        return None
    cwd = payload.get("cwd") or os.getcwd()
    eff = _effective_cwd(command, cwd)
    # Every rule scans this, not the raw command. Heredoc bodies are stripped
    # because a `sed -i /path` or `rm /path` in heredoc data is text being
    # written, not a command being run; sanctioned-tool segments are excised
    # because their remediation is the compliant move.
    scan = _excise_sanctioned(_strip_heredoc_bodies(command))
    def deny(path, project, why, what, kind=None):
        return (f"This Bash command would {what} inside "
                f"{hit_noun(project, kind)} ({path}). {why}\n\n"
                f"{hit_advice(project, kind)}\n"
                f"Reads are fine — it is the write that is refused.")

    verdict = _apply_patch_command_decision(command, eff, scan)
    if verdict:
        return verdict

    tokens = _tokens(scan)
    invocations = _git_invocations(scan, eff)

    # 0. destroying uncommitted work in a main checkout — its own class, because
    #    the loss is the human's and is not recoverable from the ref. A pin has
    #    no human WIP to lose; it answers with the pin's own reason and remedy,
    #    because `wt-rescue` is the wrong move against an externally consumed
    #    checkout.
    for target, verb, args in invocations:
        what = _wip_destroying(verb, args)
        if not what:
            continue
        hit = protected_project(target)
        if hit and is_pin(hit[2]):
            project, why, kind = hit
            return (f"`{what}` targets {target}, {hit_noun(project, kind)}. "
                    f"{why}\n\n{hit_advice(project, kind)}")
        if hit:
            project, why, _kind = hit
            return (
                f"`{what}` targets {target}, the MAIN checkout of {project}. "
                f"Uncommitted state in a main checkout is the human's "
                f"work-in-progress; an agent never discards it. {why}\n\n"
                f"Compliant moves:\n"
                f"  git -C {target} status --porcelain   # inspect, do not discard\n"
                f"  wt-rescue {target}\n"
                f"  # dirty main? run `wt-rescue` (relocates intact, restores clean)\n"
                f"  # rare surgery only: `north config guards off` — deliberate\n"
                f"  # bypass, state why, re-enable after\n"
                f"{worktree_advice(project)}")

    # 1. mutating git, wherever it points
    g = _git_decision(invocations)
    if g:
        target, verb = g
        hit = protected_project(target)
        if hit:
            project, why, kind = hit
            return deny(target, project, why, f"run `git {verb}`", kind)

    # 2. shell redirection into a protected path
    for raw in _redirect_targets(scan):
        resolved = _resolve(os.path.expanduser(raw), eff)
        hit = protected_project(resolved)
        if hit:
            project, why, kind = hit
            return deny(resolved, project, why, "write", kind)

    # 3. in-place / destination-taking commands, per SEGMENT.
    #    `cp`/`mv`/`ln` take their destination as the LAST argument — but only
    #    within their own command. Scanning to the end of a compound command
    #    made `ln -s a b; echo "(none = clean)"` treat the echo's text as ln's
    #    destination and deny it. Split on shell separators first.
    for segment in re.split(SEGMENT_SPLIT, scan):
        found = _scan_write_commands(_tokens(segment), eff, deny)
        if found:
            return found

    # 4. an interpreter fed a heredoc, while cwd is a protected checkout. The
    #    written path lives inside the script and cannot be parsed out, so this
    #    is refused on cwd alone; running it from elsewhere with absolute paths
    #    is unaffected.
    hit = protected_project(eff)
    if hit and "<<" in scan:
        for tok in tokens:
            if os.path.basename(tok) in INTERPRETERS:
                return deny(eff, hit[0], hit[1],
                            "run an interpreter script with its working directory",
                            hit[2])
    return None


def _scan_write_commands(tokens, eff, deny):
    lead = os.path.basename(tokens[0]) if tokens else ""
    package_manager = lead in PACKAGE_MANAGERS
    for i, tok in enumerate(tokens):
        base = os.path.basename(tok)
        if package_manager and i > 0:
            continue
        args = [a for a in tokens[i + 1:]
                if not a.startswith("-") and not _redirection(a)]
        if base == "sed" and any(a.startswith("-i") for a in tokens[i + 1:]):
            for a in args[1:]:
                hit = protected_project(_resolve(os.path.expanduser(a), eff))
                if hit:
                    return deny(a, hit[0], hit[1], "edit in place", hit[2])
        elif base in WRITE_COMMANDS or base in DESTRUCTIVE_COMMANDS:
            for a in args:
                resolved = _resolve(os.path.expanduser(a), eff)
                hit = protected_project(resolved)
                if hit:
                    # A gitignored build artifact inside a PIN is still the pin's:
                    # it is protected because a consumer reads the tree, not
                    # because git tracks the bytes.
                    if (base in DESTRUCTIVE_COMMANDS and not is_pin(hit[2])
                            and _tracks_nothing(resolved)):
                        continue
                    return deny(a, hit[0], hit[1], f"run `{base}`", hit[2])
        elif base in COPY_COMMANDS and args:
            dest = args[-1]
            hit = protected_project(_resolve(os.path.expanduser(dest), eff))
            if hit:
                return deny(dest, hit[0], hit[1], f"run `{base}` into", hit[2])
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
