import { afterAll, beforeAll, describe, expect, setDefaultTimeout, test } from "bun:test";
import { execFileSync } from "node:child_process";
import {
  existsSync, mkdtempSync, realpathSync, rmSync, writeFileSync,
} from "node:fs";
import { basename, join } from "node:path";
import { tmpdir } from "node:os";
import {
  harvestWorktreeBranch,
  provisionWorktree,
  rollbackProvisionedWorktree,
  worktreeFinalize,
  type ProvisionedWorktree,
  type WorktreeAllocationEvent,
  type WorktreeAllocationRegistration,
  type WorktreeAllocationWriter,
} from "../src/worktree";

setDefaultTimeout(30_000);

interface Capture {
  registrations: WorktreeAllocationRegistration[];
  events: Array<{ subject: string; event: WorktreeAllocationEvent }>;
}

function captureWriter(
  capture: Capture,
  failRegister = false,
): WorktreeAllocationWriter {
  return {
    register(registration) {
      if (failRegister) throw new Error("injected registration refusal");
      capture.registrations.push(structuredClone(registration));
    },
    event(subject, event) {
      capture.events.push({ subject, event: structuredClone(event) });
    },
  };
}

function git(repo: string, ...args: string[]): string {
  return execFileSync("git", ["-C", repo, ...args], { encoding: "utf8" }).trim();
}

function initializedRepo(root: string, name: string): string {
  const repo = join(root, name);
  execFileSync("git", ["init", "-q", "-b", "main", repo]);
  git(repo, "config", "user.email", "north-test@example.invalid");
  git(repo, "config", "user.name", "North test");
  writeFileSync(join(repo, "tracked.txt"), "base\n");
  git(repo, "add", "tracked.txt");
  git(repo, "commit", "-qm", "base");
  return repo;
}

function ownership(id: string, capture: Capture) {
  return {
    runId: `run:${id}-00000000-0000-4000-8000-000000000000`,
    thread: "019f8a82-3dce-7418-b2c0-fc6184fc79c6",
    concern: "concern-1784735694797-a27c",
    provider: "auto" as const,
    writer: captureWriter(capture),
  };
}

let root: string;
let repo: string;

beforeAll(() => {
  root = mkdtempSync(join(tmpdir(), "north-worktree-allocation-"));
  repo = initializedRepo(root, `standalone-${process.pid}`);
});

afterAll(() => {
  try { rmSync(root, { recursive: true, force: true }); } catch {}
});

describe("physical allocation registration", () => {
  test("standalone repository publishes the exact content-free ownership projection before provisioning", () => {
    const capture: Capture = { registrations: [], events: [] };
    const id = `standalone-${process.pid}`;
    const lease = provisionWorktree(id, { repoRoot: repo, ...ownership(id, capture) });
    const registration = capture.registrations[0];

    expect(registration.repositoryLayout).toBe("standalone");
    expect(registration.gitCommonDir).toBe(realpathSync(join(repo, ".git")));
    expect(registration.sourceRoot).toBe(realpathSync(repo));
    expect(registration.worktree).toBe(lease.path);
    expect(registration.durableRef).toBe(`refs/heads/lane-${id}`);
    expect(registration.baseOid).toBe(git(repo, "rev-parse", "HEAD"));
    expect(registration.headOid).toBe(registration.baseOid);
    expect(registration.run).toStartWith("@run:");
    expect(registration.agent).toBe(`@agent:${id}`);
    expect(registration.thread).toBe("@019f8a82-3dce-7418-b2c0-fc6184fc79c6");
    expect(registration.concern).toBe("@concern-1784735694797-a27c");
    expect(registration.lease.enforcement).toBe("phase-1-record-only");
    expect(registration.providerAuthorityProfile).toEqual({
      version: 1, phase: "requested", provider: "auto", target: "unresolved",
      authMode: "unresolved", profile: "unresolved",
    });
    expect(capture.events.map(({ event }) => event.type)).toEqual(["provisioned"]);
    expect(Object.keys(registration).sort()).toEqual([
      "agent", "allocationNonce", "baseOid", "concern", "durableRef", "event",
      "gitCommonDir", "headOid", "lease", "providerAuthorityProfile",
      "repositoryIdentity", "repositoryLayout", "run", "sourceRoot", "subject",
      "thread", "version", "worktree",
    ]);

    worktreeFinalize(id, "ran", { ...lease });
    expect(capture.events.at(-1)?.event).toMatchObject({
      type: "quarantined",
      resourceState: "quarantined",
      error: { code: "manual_reclamation_required", phase: "finalize" },
    });
    expect(existsSync(lease.path)).toBe(true);
    rmSync(lease.path, { recursive: true, force: true });
  });

  test("linked source retains physical common-dir identity while naming the exact linked root", () => {
    const source = join(root, `linked-source-${process.pid}`);
    git(repo, "worktree", "add", "-q", "-b", `linked-source-${process.pid}`, source, "HEAD");
    const capture: Capture = { registrations: [], events: [] };
    const id = `linked-${process.pid}`;
    const lease = provisionWorktree(id, { repoRoot: source, ...ownership(id, capture) });
    const registration = capture.registrations[0];

    expect(registration.repositoryLayout).toBe("linked");
    expect(registration.sourceRoot).toBe(realpathSync(source));
    expect(registration.gitCommonDir).toBe(realpathSync(join(repo, ".git")));
    expect(registration.repositoryIdentity).toStartWith("north:git-common-dir-sha256:v1:");

    worktreeFinalize(id, "ran", { ...lease });
    rmSync(lease.path, { recursive: true, force: true });
    git(repo, "worktree", "remove", source);
    git(repo, "branch", "-d", `linked-source-${process.pid}`);
  });

  test("two independently admitted allocations have collision-free nonce, ref, path, and ownership", async () => {
    const left: Capture = { registrations: [], events: [] };
    const right: Capture = { registrations: [], events: [] };
    const leftId = `concurrent-left-${process.pid}`;
    const rightId = `concurrent-right-${process.pid}`;
    const [leftLease, rightLease] = await Promise.all([
      Promise.resolve().then(() => provisionWorktree(leftId, {
        repoRoot: repo, ...ownership(leftId, left),
      })),
      Promise.resolve().then(() => provisionWorktree(rightId, {
        repoRoot: repo, ...ownership(rightId, right),
      })),
    ]);

    expect(left.registrations[0].allocationNonce).not.toBe(right.registrations[0].allocationNonce);
    expect(left.registrations[0].subject).not.toBe(right.registrations[0].subject);
    expect(leftLease.path).not.toBe(rightLease.path);
    expect(leftLease.allocation.durableRef).not.toBe(rightLease.allocation.durableRef);
    expect(existsSync(leftLease.path)).toBe(true);
    expect(existsSync(rightLease.path)).toBe(true);

    worktreeFinalize(leftId, "ran", { ...leftLease });
    worktreeFinalize(rightId, "ran", { ...rightLease });
    for (const lease of [leftLease, rightLease])
      rmSync(lease.path, { recursive: true, force: true });
  });
});

describe("atomic failure and recovery", () => {
  test("registration refusal creates neither worktree path nor durable ref", () => {
    const capture: Capture = { registrations: [], events: [] };
    const id = `register-refusal-${process.pid}`;
    const path = `/tmp/${basename(repo)}-lane-${id}`;
    expect(() => provisionWorktree(id, {
      repoRoot: repo,
      ...ownership(id, capture),
      writer: captureWriter(capture, true),
    })).toThrow("injected registration refusal");
    expect(existsSync(path)).toBe(false);
    expect(git(repo, "branch", "--list", `lane-${id}`)).toBe("");
    expect(capture.events).toEqual([]);
  });

  test("durable-ref collision is recorded as an exact absent rollback without deleting the foreign ref", () => {
    const capture: Capture = { registrations: [], events: [] };
    const id = `ref-collision-${process.pid}`;
    const branch = `lane-${id}`;
    git(repo, "branch", branch, "HEAD");

    expect(() => provisionWorktree(id, { repoRoot: repo, ...ownership(id, capture) }))
      .toThrow("resource=quarantined");
    expect(capture.events.map(({ event }) => event.type)).toEqual(["quarantined"]);
    expect(capture.events[0].event.error).toEqual({
      code: "durable_ref_collision", phase: "physical_preflight",
    });
    expect(capture.events[0].event.recovery?.action).toBe("inspect-and-salvage");
    expect(git(repo, "branch", "--list", branch)).toContain(branch);
    expect(existsSync(`/tmp/${basename(repo)}-${branch}`)).toBe(false);

    git(repo, "branch", "-d", branch);
  });

  test("pre-provider admission rollback preserves a clean resource for manual reclamation", () => {
    const capture: Capture = { registrations: [], events: [] };
    const id = `admission-rollback-${process.pid}`;
    const lease = provisionWorktree(id, { repoRoot: repo, ...ownership(id, capture) });
    rollbackProvisionedWorktree(id, lease);

    expect(capture.events.at(-1)?.event).toMatchObject({
      type: "quarantined",
      resourceState: "quarantined",
      error: { code: "admission_aborted", phase: "admission_rollback" },
    });
    expect(existsSync(lease.path)).toBe(true);
    // The lane branch lives in the CLONE's own ref space, not the canonical repo.
    expect(git(lease.path, "branch", "--list", lease.branch)).toContain(lease.branch);
    rmSync(lease.path, { recursive: true, force: true });
  });

  test("dirty pre-provider rollback preserves and queryably quarantines with machine recovery", () => {
    const capture: Capture = { registrations: [], events: [] };
    const id = `admission-quarantine-${process.pid}`;
    const lease = provisionWorktree(id, { repoRoot: repo, ...ownership(id, capture) });
    writeFileSync(join(lease.path, "uncommitted.txt"), "salvage\n");
    const priorNorthBin = process.env.NORTH_BIN;
    process.env.NORTH_BIN = "/bin/true";
    try { rollbackProvisionedWorktree(id, lease); }
    finally {
      if (priorNorthBin === undefined) delete process.env.NORTH_BIN;
      else process.env.NORTH_BIN = priorNorthBin;
    }

    expect(capture.events.at(-1)?.event).toMatchObject({
      type: "quarantined",
      resourceState: "quarantined",
      error: { code: "worktree_dirty", phase: "admission_rollback" },
      recovery: {
        action: "inspect-and-salvage",
        resource: lease.path,
        durableRef: lease.allocation.durableRef,
      },
    });
    expect(existsSync(lease.path)).toBe(true);

    rmSync(lease.path, { recursive: true, force: true });
  });
});

describe("commit capability under a workspace-only write sandbox", () => {
  // Reproduction-then-proof for defect 019f8eaa. A linked worktree's commit needs
  // index.lock under the PARENT repo's .git/worktrees/<id>/ — outside a provider's
  // workspace-write sandbox (Codex `workspace-write`, no --add-dir) — so `git commit`
  // died with "index.lock ... read-only". We SIMULATE that sandbox by making the
  // canonical repo's .git READ-ONLY, then prove a provisioned CLONE still commits (its
  // whole git-dir is inside the workspace). The pre-fix linked-worktree mechanism fails
  // this exact assertion; the clone passes it.
  test("a provisioned clone commits on its lane branch while the canonical .git is read-only", () => {
    const capture: Capture = { registrations: [], events: [] };
    const id = `commit-capability-${process.pid}`;
    const lease = provisionWorktree(id, { repoRoot: repo, ...ownership(id, capture) });

    // git-dir is INSIDE the workspace, not the canonical .git.
    const gitDir = git(lease.path, "rev-parse", "--absolute-git-dir");
    expect(gitDir.startsWith(realpathSync(lease.path))).toBe(true);
    expect(gitDir).not.toContain(realpathSync(join(repo, ".git")));

    const canonicalHeadBefore = git(repo, "rev-parse", "HEAD");
    const canonicalGit = join(repo, ".git");
    execFileSync("chmod", ["-R", "a-w", canonicalGit]);
    try {
      writeFileSync(join(lease.path, "worker-edit.txt"), "written inside the sandbox\n");
      git(lease.path, "add", "worker-edit.txt");
      // No throw == commit succeeded under the read-only-canonical restriction.
      git(lease.path, "-c", "user.email=lane@test.invalid", "-c", "user.name=lane",
        "commit", "-qm", "lane commit under sandbox");
      expect(git(lease.path, "rev-parse", "--abbrev-ref", "HEAD")).toBe(`lane-${id}`);
      expect(git(lease.path, "log", "-1", "--pretty=%s")).toBe("lane commit under sandbox");
      // Isolation held: the canonical checkout HEAD never moved.
      expect(git(repo, "rev-parse", "HEAD")).toBe(canonicalHeadBefore);
    } finally {
      execFileSync("chmod", ["-R", "u+w", canonicalGit]);
    }

    // Push is neutered to the sentinel; fetch still points at the canonical repo.
    expect(git(lease.path, "remote", "get-url", "--push", "origin"))
      .toBe("north-disabled://managed-clone-no-push");
    expect(realpathSync(git(lease.path, "remote", "get-url", "origin")))
      .toBe(realpathSync(repo));

    rmSync(lease.path, { recursive: true, force: true });
  });
});

describe("terminal harvest of a lane's committed work (defect B)", () => {
  // A lane clone's commits live ONLY in its own /tmp-resident object store, so a lane that
  // committed real work and then died left work the documented landing step ("ff-merge
  // lane-<id> in the main tree") could not even name. Harvest fetches the lane branch back
  // into the canonical repository — a local fetch, never a push.
  function laneCommit(loc: ProvisionedWorktree, body: string): string {
    writeFileSync(join(loc.path, "landed.txt"), body);
    git(loc.path, "add", "landed.txt");
    git(loc.path, "-c", "user.email=lane@test.invalid", "-c", "user.name=lane",
      "commit", "-qm", `lane work: ${body.trim()}`);
    return git(loc.path, "rev-parse", "HEAD");
  }

  function withNeutralNorthBin<T>(run: () => T): T {
    const prior = process.env.NORTH_BIN;
    process.env.NORTH_BIN = "/bin/true";
    try { return run(); }
    finally {
      if (prior === undefined) delete process.env.NORTH_BIN;
      else process.env.NORTH_BIN = prior;
    }
  }

  test("a died lane's committed branch reaches the canonical repository, and nothing is pushed", () => {
    const capture: Capture = { registrations: [], events: [] };
    const id = `harvest-died-${process.pid}`;
    const lease = provisionWorktree(id, { repoRoot: repo, ...ownership(id, capture) });
    const laneHead = laneCommit(lease, "harvested work\n");
    const canonicalHeadBefore = git(repo, "rev-parse", "HEAD");

    // The pre-fix state: the canonical repo cannot name the lane's commits at all.
    expect(git(repo, "branch", "--list", lease.branch)).toBe("");

    withNeutralNorthBin(() => worktreeFinalize(id, "died", { ...lease }));

    expect(git(repo, "rev-parse", "--verify", `refs/heads/${lease.branch}`)).toBe(laneHead);
    expect(git(repo, "cat-file", "-t", laneHead)).toBe("commit");
    // Harvest is additive only: the canonical working tree and HEAD never move, and the
    // lane's clone is still preserved for salvage.
    expect(git(repo, "rev-parse", "HEAD")).toBe(canonicalHeadBefore);
    expect(existsSync(lease.path)).toBe(true);
    expect(git(lease.path, "remote", "get-url", "--push", "origin"))
      .toBe("north-disabled://managed-clone-no-push");

    git(repo, "branch", "-D", lease.branch);
    rmSync(lease.path, { recursive: true, force: true });
  });

  test("harvest reports its observation: commits, nothing-committed, and a refused divergence", () => {
    const capture: Capture = { registrations: [], events: [] };
    const bare = `harvest-observed-${process.pid}`;
    const empty = provisionWorktree(bare, { repoRoot: repo, ...ownership(bare, capture) });
    expect(harvestWorktreeBranch(bare, empty)).toMatchObject({
      status: "nothing-committed", commits: 0, ref: `refs/heads/${empty.branch}`,
    });
    // Nothing committed => no litter in the canonical ref space.
    expect(git(repo, "branch", "--list", empty.branch)).toBe("");
    rmSync(empty.path, { recursive: true, force: true });

    const id = `harvest-counted-${process.pid}`;
    const lease = provisionWorktree(id, { repoRoot: repo, ...ownership(id, capture) });
    laneCommit(lease, "first\n");
    const laneHead = laneCommit(lease, "second\n");
    expect(harvestWorktreeBranch(id, lease)).toMatchObject({
      status: "harvested", commits: 2, headOid: laneHead,
    });
    // Idempotent: re-harvesting the same head is a no-op fast-forward, not a failure.
    expect(harvestWorktreeBranch(id, lease)).toMatchObject({ status: "harvested" });

    // A canonical ref that diverged from the lane is NEVER overwritten — the harvest is
    // reported unavailable and the existing commits stand.
    const divergent = `harvest-divergent-${process.pid}`;
    const other = provisionWorktree(divergent, { repoRoot: repo, ...ownership(divergent, capture) });
    laneCommit(other, "lane side\n");
    writeFileSync(join(repo, "canonical.txt"), "canonical side\n");
    git(repo, "add", "canonical.txt");
    git(repo, "-c", "user.email=main@test.invalid", "-c", "user.name=main",
      "commit", "-qm", "canonical divergence");
    git(repo, "branch", other.branch, "HEAD");
    const canonicalRef = git(repo, "rev-parse", `refs/heads/${other.branch}`);
    expect(harvestWorktreeBranch(divergent, other).status).toBe("unavailable");
    expect(git(repo, "rev-parse", `refs/heads/${other.branch}`)).toBe(canonicalRef);

    git(repo, "branch", "-D", lease.branch);
    git(repo, "branch", "-D", other.branch);
    rmSync(lease.path, { recursive: true, force: true });
    rmSync(other.path, { recursive: true, force: true });
  });
});
