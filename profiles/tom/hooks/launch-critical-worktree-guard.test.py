#!/usr/bin/env python3
"""The guard must refuse the Bash calls that actually breached the primaries.

The DENY cases in the first section are verbatim command shapes an agent ran
against a launch-critical primary on 2026-07-29 while the guard was live and
wired. None of them fired, because the guard only inspected
`tool_input.file_path`, which a Bash call does not have. These cases exist so
that hole cannot reopen. Later sections cover every protected `main` and the
WIP-destroying git verbs.

The ALLOW cases matter just as much: a guard that also blocks reads, or blocks
`git worktree add` — the very escape route its deny message recommends — traps
the agent with no compliant move, and the next person turns the guard off.
"""

import json
import os
import shutil
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
DECIDE = os.path.join(HERE, "lib", "launch_critical_decide.py")

failures = []
checks = 0


def run(payload, code_root=None):
    env = dict(os.environ)
    if code_root:
        env["LAUNCH_CRITICAL_CODE_ROOT"] = code_root
    else:
        env.pop("LAUNCH_CRITICAL_CODE_ROOT", None)
    p = subprocess.run([sys.executable, DECIDE], input=json.dumps(payload),
                       capture_output=True, text=True, timeout=30, env=env)
    out = p.stdout.strip()
    if not out:
        return None
    return json.loads(out)["hookSpecificOutput"]["permissionDecisionReason"]


def bash(command, cwd="/home/tom"):
    return {"tool_name": "Bash", "tool_input": {"command": command}, "cwd": cwd}


def patch(envelope, cwd="/home/tom", key="input", tool="apply_patch", extra=None):
    ti = {key: envelope}
    if extra:
        ti.update(extra)
    return {"tool_name": tool, "tool_input": ti, "cwd": cwd}


ENV = "*** Begin Patch\n*** {verb} File: {path}\n{body}*** End Patch"


def check(label, condition):
    global checks
    checks += 1
    if condition:
        print("  PASS ", label)
    else:
        failures.append(label)
        print("  FAIL ", label)


NORTH = "/home/tom/code/north/main"
FRAM = "/home/tom/code/fram/main"

print("--- must DENY: what actually happened on 2026-07-29 ---")

check("heredoc patch with cwd in the north primary",
      run(bash("python3 - <<'PYEOF'\nopen('cli/deployed-cli.clj','w')\nPYEOF", cwd=NORTH)))

check("git add from inside the north primary",
      run(bash("git add cli/deployed-cli.clj", cwd=NORTH)))

check("git commit from inside the north primary",
      run(bash("git commit -q -m 'x'", cwd=NORTH)))

check("git reset --hard against the fram primary via -C",
      run(bash("git -C /home/tom/code/fram/main reset --hard origin/main")))

check("git add of many paths from inside the fram primary",
      run(bash("FILES=$(cat list); git add $FILES", cwd=FRAM)))

check("git push from inside the primary",
      run(bash("git push origin main", cwd=NORTH)))

check("redirection into a primary file",
      run(bash("echo x > /home/tom/code/north/main/cli/x.clj")))

check("sed -i against a primary file",
      run(bash("sed -i s/a/b/ /home/tom/code/fram/main/coord_daemon.clj")))

check("cp INTO a primary",
      run(bash("cp /tmp/x.clj /home/tom/code/north/main/cli/x.clj")))

check("rm inside a primary",
      run(bash("rm /home/tom/code/beagle/main/bin/beagle")))

check("cd into a primary then write, in one command",
      run(bash("cd /home/tom/code/north/main && echo x > cli/x.clj")))

check("the deny names the project", "north" in (run(bash("git add .", cwd=NORTH)) or ""))
check("the deny gives the worktree escape route",
      "worktree add" in (run(bash("git add .", cwd=NORTH)) or ""))

print("--- must ALLOW: reads, and the sanctioned way out ---")

check("git log against a primary", run(bash("git -C /home/tom/code/north/main log --oneline -1")) is None)
check("git status against a primary", run(bash("git status --porcelain", cwd=NORTH)) is None)
check("git diff against a primary", run(bash("git -C /home/tom/code/fram/main diff --stat")) is None)
check("grep inside a primary", run(bash("grep -rn foo cli/", cwd=NORTH)) is None)
check("cat a primary file", run(bash("cat /home/tom/code/north/main/cli/coord.clj")) is None)

check("git worktree add FROM the primary is the escape route, never blocked",
      run(bash("git -C /home/tom/code/north/main worktree add /home/tom/code/north/wt-x -b x")) is None)
check("git fetch INTO the primary is allowed",
      run(bash("git -C /home/tom/code/north/main fetch /home/tom/code/north/wt-x x:refs/heads/main")) is None)

# THE LANDING PATH MUST WORK. `fetch <wt> <branch>:refs/heads/main` fails when
# main is checked out — which it always is under this layout — so --ff-only
# merge/pull is the only way work can reach main. A guard that blocks the one
# compliant landing move is a guard that gets switched off.
check("merge --ff-only into the primary is the landing path",
      run(bash("git -C /home/tom/code/north/main merge --ff-only feature")) is None)
check("pull --ff-only into the primary is allowed",
      run(bash("git -C /home/tom/code/north/main pull --ff-only")) is None)
check("a BARE merge into the primary is still denied (can conflict, can dirty)",
      run(bash("git -C /home/tom/code/north/main merge feature")))
check("a BARE pull into the primary is still denied",
      run(bash("git -C /home/tom/code/north/main pull")))

# FALSE POSITIVES. Each of these was a real denial this guard produced against
# legitimate work on 2026-07-29, and each one is a reason someone would switch
# it off. A guard that cries wolf is worse than no guard.
check("an arrow inside a quoted string is not a redirect",
      run(bash('echo "north/bin -> needs compat"', cwd=NORTH)) is None)
check("fd duplication (2>&1) opens no file",
      run(bash("grep -rn foo cli/ 2>&1 | head", cwd=NORTH)) is None)
check("a redirect inside a HEREDOC BODY is data, not shell syntax",
      run(bash("cat > /tmp/t.py <<'EOF'\ncheck('echo x > /home/tom/code/north/main/cli/x.clj')\nEOF")) is None)
check("sed -i inside a heredoc body is data, not a command",
      run(bash("cat > /tmp/t.sh <<'EOF'\nsed -i s/a/b/ /home/tom/code/fram/main/coord_daemon.clj\nEOF")) is None)

# ...while the real forms are still refused.
check("a REAL redirect into a primary is still denied",
      run(bash("echo x > /home/tom/code/north/main/cli/zz.clj")))
check("a REAL sed -i on a primary is still denied",
      run(bash("sed -i s/a/b/ /home/tom/code/fram/main/coord_daemon.clj")))

check("redirect to /tmp while cwd is a primary is fine",
      run(bash("grep foo cli/x.clj > /tmp/out", cwd=NORTH)) is None)

print("--- the wt-* carve-out (both layouts) ---")

check("writing in a wt- worktree is allowed",
      run(bash("git add .", cwd="/home/tom/code/north/wt-abc")) is None)
check("heredoc in a wt- worktree is allowed",
      run(bash("python3 - <<'EOF'\npass\nEOF", cwd="/home/tom/code/north/wt-abc")) is None)
check("a path under <project>/main IS protected (target layout)",
      run(bash("git add x", cwd="/home/tom/code/north/main")))
check("unrelated repos are untouched",
      run(bash("git commit -m x", cwd="/home/tom/code/some-other-project")) is None)
check("a sibling like north-data must not match",
      run(bash("git add x", cwd="/home/tom/code/north-data")) is None)

# The CONTAINER root is not a checkout: it holds only main/ and wt-*/, so a
# symlink or scratch file there cannot dirty anything. Denying it left no
# compliant way to place a compatibility symlink during the migration.
check("the container root itself is writable",
      run(bash("ln -s main/orchestration orchestration-probe",
               cwd="/home/tom/code/north")) is None)
check("but <project>/main is still protected",
      run(bash("touch x", cwd="/home/tom/code/north/main")) is not False)

print("--- Edit/Write behaviour is unchanged ---")

check("Edit into a primary is denied",
      run({"tool_name": "Edit",
           "tool_input": {"file_path": "/home/tom/code/north/main/cli/x.clj"}}))
check("Edit into a wt- worktree is allowed",
      run({"tool_name": "Edit",
           "tool_input": {"file_path": "/home/tom/code/north/wt-a/cli/x.clj"}}) is None)
check("Edit outside ~/code is allowed",
      run({"tool_name": "Edit", "tool_input": {"file_path": "/tmp/x.clj"}}) is None)

print("--- every container's main, not only the launch-critical ones ---")

# A fixture code root: dynamic detection must cover a project the guard has
# never heard of, and must not sweep in the near-misses that live beside one.
FIX = tempfile.mkdtemp(prefix="lc-guard-")
ROOT = os.path.join(FIX, "code")
for rel in ("proj/main/.git", "proj/wt-x/.git", "client/msa/app/main/.git",
            "reference/upstream/main/.git", "runtime-data/.git",
            "beagle/.git", "plain-dir"):
    os.makedirs(os.path.join(ROOT, rel), exist_ok=True)

PROJ = os.path.join(ROOT, "proj", "main")
CLIENT = os.path.join(ROOT, "client", "msa", "app", "main")


def fixture(command, cwd=None):
    return run(bash(command, cwd=cwd or ROOT), code_root=ROOT)


check("an unheard-of project's main is protected",
      fixture("git commit -m x", cwd=PROJ))
check("a client project's nested main is protected",
      fixture("git add .", cwd=CLIENT))
check("the deny names the nested container",
      "client/msa/app" in (fixture("git add .", cwd=CLIENT) or ""))
check("its wt- worktree is not",
      fixture("git commit -m x", cwd=os.path.join(ROOT, "proj", "wt-x")) is None)
check("a data dir with a bare .git and no main/ stays writable",
      fixture("git commit -m x", cwd=os.path.join(ROOT, "runtime-data")) is None)
check("a directory that is no checkout at all is untouched",
      fixture("rm -rf junk", cwd=os.path.join(ROOT, "plain-dir")) is None)
check("~/code/reference is read-only context, never this guard's business",
      fixture("cat notes.md",
              cwd=os.path.join(ROOT, "reference", "upstream", "main")) is None)
check("a launch-critical container that IS the checkout (pre-migration)",
      fixture("git add .", cwd=os.path.join(ROOT, "beagle")))
check("Edit into an unheard-of project's main is denied",
      run({"tool_name": "Edit",
           "tool_input": {"file_path": os.path.join(PROJ, "src/x.py")}},
          code_root=ROOT))
check("Edit into ~/code/reference is allowed",
      run({"tool_name": "Edit",
           "tool_input": {"file_path": os.path.join(
               ROOT, "reference/upstream/main/x.py")}},
          code_root=ROOT) is None)

shutil.rmtree(FIX, ignore_errors=True)

print("--- destroying human WIP in a main checkout ---")

NIXOS = "/home/tom/code/nixos-config/main"

check("reset --hard against nixos-config/main (the 2026-07-30 near-miss)",
      run(bash("git -C /home/tom/code/nixos-config/main reset --hard HEAD~1")))
check("the deny names a compliant move",
      "status --porcelain" in (run(bash(f"git -C {NIXOS} reset --hard")) or ""))
check("reset --merge is the same loss", run(bash(f"git -C {NIXOS} reset --merge")))
check("stash from inside a main", run(bash("git stash", cwd=NIXOS)))
check("stash push with a message", run(bash("git stash push -m wip", cwd=NORTH)))
check("stash pop", run(bash("git stash pop", cwd=NORTH)))
check("checkout -- <path> discards the edit",
      run(bash("git checkout -- cli/x.clj", cwd=NORTH)))
check("restore of the working tree", run(bash("git restore cli/x.clj", cwd=NORTH)))
check("clean -fd", run(bash("git clean -fd", cwd=NORTH)))
check("clean -ffdx", run(bash(f"git -C {NIXOS} clean -ffdx")))

# The hole this class closes: the old scan stopped at the FIRST git call, so a
# sanctioned verb ahead of a destructive one vouched for the whole line.
check("a sanctioned verb earlier in the line does not shield a reset --hard",
      run(bash(f"git -C {NORTH} worktree add /tmp/wt-y -b y && "
               f"git -C {NORTH} reset --hard origin/main")))
check("...nor a stash after a fetch",
      run(bash(f"git -C {NORTH} fetch origin && git -C {NORTH} stash")))

check("git stash list is a read", run(bash("git stash list", cwd=NORTH)) is None)
check("git clean --dry-run is a read", run(bash("git clean -nd", cwd=NORTH)) is None)
# --staged spares the working tree, so it is not WIP destruction — but it still
# rewrites an index the human staged, so it stays denied as an ordinary mutation.
_staged = run(bash("git restore --staged cli/x.clj", cwd=NORTH))
check("git restore --staged is a mutation, not WIP destruction",
      _staged and "work-in-progress" not in _staged)

print("--- wt-rescue is the sanctioned remediation and must pass ---")

# It performs internally the cleanup denied raw. A guard that blocks the move
# its own deny message recommends leaves the lane trapped.
check("wt-rescue against a main is allowed",
      run(bash(f"wt-rescue {NIXOS}")) is None)
check("wt-rescue with no path, run from inside a main",
      run(bash("wt-rescue", cwd=NORTH)) is None)
check("wt-rescue --dry-run is allowed",
      run(bash(f"wt-rescue {NORTH} --dry-run")) is None)
check("the checkout-- denial names wt-rescue",
      "wt-rescue" in (run(bash(f"git -C {NIXOS} checkout -- .")) or ""))
check("the reset --hard denial names wt-rescue",
      "wt-rescue" in (run(bash(f"git -C {NIXOS} reset --hard")) or ""))
check("the denial names the deliberate-bypass escape",
      "north config guards off" in (run(bash("git stash", cwd=NORTH)) or ""))
check("the allowlist is per SEGMENT — a reset --hard after it still denies",
      run(bash(f"wt-rescue {NORTH} && git -C {NORTH} reset --hard origin/main")))
check("...and a plain mutation after it still denies",
      run(bash(f"wt-rescue {NORTH}; git -C {NORTH} commit -m x")))
check("a wt-rescue's own rescue worktree is a wt- destination",
      run(bash("git add .", cwd="/home/tom/code/north/wt-rescue-20260730-1600")) is None)
check("the word wt-rescue as mere text vouches for nothing",
      run(bash(f"echo wt-rescue && git -C {NORTH} reset --hard")))

check("reset --hard INSIDE a worktree is the lane's own business",
      run(bash("git reset --hard origin/main",
               cwd="/home/tom/code/north/wt-abc")) is None)
check("stash inside a worktree is fine",
      run(bash("git -C /home/tom/code/north/wt-abc stash")) is None)
check("clean -fd inside a worktree is fine",
      run(bash("git clean -fd", cwd="/home/tom/code/fram/wt-abc")) is None)

print("--- the landing flow must still run from main ---")

check("worktree remove", run(bash(
    "git -C /home/tom/code/north/main worktree remove /home/tom/code/north/wt-x")) is None)
check("worktree prune",
      run(bash("git -C /home/tom/code/north/main worktree prune")) is None)
check("branch -d", run(bash("git -C /home/tom/code/north/main branch -d slug")) is None)
check("branch -D", run(bash("git branch -D slug", cwd=NORTH)) is None)
check("safe-push", run(bash("safe-push --to main", cwd=NORTH)) is None)
check("git show", run(bash("git show --stat HEAD", cwd=NORTH)) is None)
check("the whole sequence in one command", run(bash(
    "git -C /home/tom/code/north/main merge --ff-only slug && "
    "git -C /home/tom/code/north/main branch -d slug && "
    "git -C /home/tom/code/north/main worktree prune")) is None)
check("rebase run in a WORKTREE is untouched",
      run(bash("git rebase main", cwd="/home/tom/code/north/wt-abc")) is None)
check("rebase run in main is not",
      run(bash("git rebase origin/main", cwd=NORTH)))

print("--- apply_patch envelopes: every header form ---")

add_main = ENV.format(verb="Add", path=f"{NORTH}/cli/x.clj", body="+x\n")
update_relative = ENV.format(verb="Update", path="cli/x.clj", body="@@\n-a\n+b\n")
delete_fram = ENV.format(verb="Delete", path=f"{FRAM}/x.clj", body="")
move_main = ENV.format(
    verb="Update", path="/home/tom/code/north/wt-abc/a.clj",
    body=f"*** Move to: {NORTH}/a.clj\n@@\n-a\n+b\n")
move_worktree = ENV.format(
    verb="Update", path="/home/tom/code/north/wt-abc/a.clj",
    body="*** Move to: /home/tom/code/north/wt-abc/b.clj\n@@\n-a\n+b\n")
tmp_add = ENV.format(verb="Add", path="/tmp/x.txt", body="+x\n")

check("Add File with an absolute primary target is denied", run(patch(add_main)))
check("Update File resolves a relative target against cwd",
      run(patch(update_relative, cwd=NORTH)))
check("Delete File into fram's primary is denied", run(patch(delete_fram)))
check("Move to destination alone trips primary protection", run(patch(move_main)))
check("Update and Move to within a worktree are allowed",
      run(patch(move_worktree)) is None)
check("Add File outside protected checkouts is allowed", run(patch(tmp_add)) is None)
check("a nested envelope is found recursively", run({
    "tool_name": "apply_patch",
    "tool_input": {"arguments": {"patch": add_main}},
    "cwd": "/home/tom",
}))
check("a prefixed functions.apply_patch tool is recognized",
      run(patch(add_main, tool="functions.apply_patch")))
check("an explicit file_path remains authoritative",
      run(patch(tmp_add, extra={"file_path": f"{NORTH}/cli/x.clj"})))
check("the apply_patch deny names the project",
      "north" in (run(patch(add_main)) or ""))
check("the apply_patch deny gives the worktree escape route",
      "worktree add" in (run(patch(add_main)) or ""))
malformed = "*** Begin Patch\n*** Frobnicate: x\n*** End Patch"
check("a malformed apply_patch envelope is denied", run(patch(malformed)))
check("the malformed-envelope deny is explicitly fail-closed",
      "fail-closed" in (run(patch(malformed)) or ""))
check("an apply_patch tool call without an envelope is denied",
      run(patch("no patch here")))
check("tool_input.workdir wins over payload cwd for relative targets",
      run(patch(update_relative, cwd="/tmp", extra={"workdir": NORTH})))
check("a relative target under /tmp is allowed",
      run(patch(ENV.format(verb="Update", path="x.txt", body="@@\n-a\n+b\n"),
                extra={"workdir": "/tmp"})) is None)

print("--- apply_patch through the shell ---")

protected_heredoc = (
    "apply_patch <<'EOF'\n" + add_main + "\nEOF")
worktree_envelope = ENV.format(
    verb="Update", path="/home/tom/code/north/wt-abc/cli/x.clj",
    body="@@\n-a\n+b\n")
worktree_heredoc = "apply_patch <<'EOF'\n" + worktree_envelope + "\nEOF"
check("a shell apply_patch heredoc into a primary is denied",
      run(bash(protected_heredoc)))
check("a shell apply_patch heredoc into a worktree is allowed",
      run(bash(worktree_heredoc)) is None)
check("an envelope written as pure heredoc data is allowed", run(bash(
    "cat > /tmp/t.md <<'EOF'\n*** Begin Patch\n"
    f"*** Update File: {NORTH}/x\n*** End Patch\nEOF")) is None)
check("apply_patch input redirection fails closed",
      "fail-closed" in (run(bash("apply_patch < /tmp/patch.txt")) or ""))
check("the direct apply_patch argv form is denied", run(bash([
    "apply_patch", add_main,
])))
check("the direct apply_patch argv form allows a worktree", run(bash([
    "apply_patch", worktree_envelope,
])) is None)
check("an argv shell wrapper invoking apply_patch is denied", run(bash([
    "bash", "-lc", protected_heredoc,
])))
check("an allowed envelope cannot shield a later destructive git command",
      run(bash(worktree_heredoc +
               f"\n&& git -C {NORTH} reset --hard")))
check("generic non-apply_patch argv handling remains out of scope",
      run(bash(["rm", f"{NORTH}/x"])) is None)

print("--- fail-open ---")

check("no command field is allowed", run({"tool_name": "Bash", "tool_input": {}}) is None)
check("empty payload is allowed", run({}) is None)
check("unparseable command does not crash",
      run(bash('git add "unclosed', cwd=NORTH)) is not False)

print()
if failures:
    print(f"launch-critical guard: {len(failures)} FAILED of {checks}")
    sys.exit(1)
print(f"launch-critical guard: {checks} / {checks} PASS")
