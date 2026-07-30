"""Shared path policy for the main-checkout guard.

THE LAYOUT

    ~/code/<project>/          container only — never a checkout
    ~/code/<project>/main/     the clean main checkout; agents NEVER write here
    ~/code/<project>/wt-<slug>/  every agent's working tree

EVERY `main` checkout under ~/code is protected, not only the launch-critical
ones: `main` is never edited in place anywhere, and dirty state there is the
human's. Detection is dynamic — an ancestor directory named `main` holding a
`.git` — so a project is covered the day it is cloned. Client repos nest
(~/code/client/<owner>/<project>/main) and fall out of the same rule.

The launch-critical containers keep their own reasons and their pre-migration
single-checkout handling (the container itself being the checkout).
"""

import os

# Containers whose PRIMARY breaks something beyond itself. They are protected by
# the same rule as every other main; only the reason text and the pre-migration
# whole-container handling are theirs.
LAUNCH_CRITICAL = {
    "fram": (
        "fram is the running database engine and is launch-critical. `north up`"
        " REFUSES to launch on a tracked-dirty checkout, so writing here can"
        " leave the daemon unrestartable and block every rebuild from being"
        " adopted."
    ),
    "north": (
        "north is launch-critical and `firn rebuild` builds a COMMIT SNAPSHOT, so"
        " uncommitted work here is silently absent from the generation."
    ),
    "beagle": (
        "beagle is launch-critical: it compiles north and nixos-config, and a"
        " half-edited checkout breaks builds for every other lane at once."
    ),
    "nixos-config": (
        "nixos-config is launch-critical: it is the system source and `firn"
        " rebuild` snapshots commits, so uncommitted work here never reaches a"
        " generation."
    ),
}

GENERIC_REASON = (
    "`main` is the clean checkout of a project — nothing is edited there, by"
    " anyone, and uncommitted state in it belongs to the human."
)

# Read-only context by policy; never a work destination, so never protected here.
EXCLUDED_ROOTS = ("reference",)


def code_root():
    """The ~/code root. Overridable so tests can build a fixture layout."""
    return os.path.realpath(
        os.environ.get("LAUNCH_CRITICAL_CODE_ROOT") or os.path.expanduser("~/code"))


def _within(path, root):
    """Is PATH the directory ROOT or something beneath it?"""
    return path == root or path.startswith(root + os.sep)


def _reason(container):
    return LAUNCH_CRITICAL.get(container, GENERIC_REASON)


def protected_project(path):
    """(container, reason) when PATH is inside a protected checkout, else None.

    CONTAINER is relative to the code root, so a nested client container
    ("client/<owner>/<project>") names itself correctly in the advice.
    """
    if not isinstance(path, str) or not path:
        return None
    try:
        real = os.path.realpath(path)
    except Exception:
        return None

    root = code_root()
    if not _within(real, root):
        return None

    rest = real[len(root):].strip(os.sep)
    parts = [p for p in rest.split(os.sep) if p] if rest else []
    if not parts or parts[0] in EXCLUDED_ROOTS:
        return None

    # Worktrees are the sanctioned destination, always.
    if any(p.startswith("wt-") for p in parts):
        return None

    # The nearest ancestor named `main` that is actually a checkout. A `main`
    # directly under the code root has no container and is not this layout.
    for i, part in enumerate(parts):
        if part != "main" or i == 0:
            continue
        if os.path.exists(os.path.join(root, *(parts[:i + 1] + [".git"]))):
            container = os.sep.join(parts[:i])
            return (container, _reason(container))

    # Pre-migration: a launch-critical container that IS the checkout. Scoped to
    # that set on purpose — generalising it would protect ~/code/<data-dir>,
    # which is runtime state that must stay writable.
    head = parts[0]
    if (head in LAUNCH_CRITICAL
            and not os.path.exists(os.path.join(root, head, "main", ".git"))
            and os.path.exists(os.path.join(root, head, ".git"))):
        return (head, LAUNCH_CRITICAL[head])
    return None


def worktree_advice(project):
    """The exact commands to work correctly, in the canonical layout."""
    container = os.path.join(code_root(), project)
    return (
        "Work in a worktree, then land through a ref:\n"
        f"  git -C {container}/main worktree add {container}/wt-<slug> -b <slug>\n"
        f"  # edit + commit in {container}/wt-<slug>, then land it:\n"
        f"  git -C {container}/main merge --ff-only <slug>\n"
        f"  # (plain `fetch <wt> <b>:refs/heads/main` cannot be used: main is checked out)\n"
    )
