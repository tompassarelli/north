// Trust anchor for managed-spawn Git discovery. The security invariant under test:
// a candidate is accepted ONLY when its real canonical executable lives in the
// immutable /nix/store and is executable. Entry hints (wrapper injection, system
// and per-user Nix profiles) are conveniences; the canonical proof is the guard.
// Ambient $PATH and writable shim locations are never trusted, and absent proof
// resolution fails closed. These are the exact behaviors a managed spawn depends
// on before it can publish lane identity.
import { test, expect, describe, beforeEach, afterEach } from "bun:test";
import {
  chmodSync, mkdirSync, mkdtempSync, realpathSync, rmSync, symlinkSync, writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { trustedGitExecutable } from "../src/trusted-runtime";

const STORE_GIT = /^\/nix\/store\/[0-9a-z]{32}-git(?:-[^/]+)?\/bin\/git$/;

// A genuine canonical /nix/store git present on this host. Tests that need a real
// store target (symlink chains, positive canonical acceptance) build atop it,
// because a temp dir can never be a real /nix/store path.
function realStoreGit(): string {
  const candidates = [
    process.env.NORTH_GIT_BIN,
    `${process.env.HOME}/.nix-profile/bin/git`,
    "/run/current-system/sw/bin/git",
    "/etc/profiles/per-user/" + (process.env.USER ?? "") + "/bin/git",
  ];
  for (const candidate of candidates) {
    if (!candidate) continue;
    try {
      const real = realpathSync(candidate);
      if (STORE_GIT.test(real)) return real;
    } catch {
      // keep looking
    }
  }
  throw new Error("test host exposes no canonical /nix/store git");
}

describe("trustedGitExecutable — canonical store proof", () => {
  const git = realStoreGit();
  let scratch: string;

  beforeEach(() => {
    scratch = mkdtempSync(join(tmpdir(), "trusted-git-"));
  });
  afterEach(() => {
    rmSync(scratch, { recursive: true, force: true });
  });

  test("accepts a canonical Nix-store executable given directly", () => {
    expect(trustedGitExecutable([git])).toBe(git);
  });

  test("accepts a supported multi-hop symlink chain into the store", () => {
    // profile -> intermediate -> real store git, the real current-system/profile shape.
    const hop2 = join(scratch, "git");
    const hop1 = join(scratch, "profile-git");
    symlinkSync(git, hop2);
    symlinkSync(hop2, hop1);
    expect(trustedGitExecutable([hop1])).toBe(git);
  });

  test("skips a broken candidate and resolves a later valid one", () => {
    expect(trustedGitExecutable([join(scratch, "absent"), git])).toBe(git);
  });

  test("rejects a non-store candidate even when it is executable", () => {
    const shim = join(scratch, "git");
    writeFileSync(shim, "#!/bin/sh\nexit 0\n");
    chmodSync(shim, 0o755);
    expect(() => trustedGitExecutable([shim])).toThrow(
      "trusted Nix-store Git executable unavailable",
    );
  });

  test("rejects a writable shim whose path merely CONTAINS /nix/store", () => {
    // A store-look-alike under a writable root. Its real canonical path is the
    // temp dir, not the immutable store, so the anchor is the absolute canonical
    // prefix — never a substring.
    const fake = join(scratch, "nix", "store", "0".repeat(32) + "-git-2.9.9", "bin", "git");
    mkdirSync(dirname(fake), { recursive: true });
    writeFileSync(fake, "#!/bin/sh\nexit 0\n");
    chmodSync(fake, 0o755);
    expect(() => trustedGitExecutable([fake])).toThrow(
      "trusted Nix-store Git executable unavailable",
    );
  });

  test("rejects a missing executable", () => {
    expect(() => trustedGitExecutable([join(scratch, "does-not-exist")])).toThrow(
      "trusted Nix-store Git executable unavailable",
    );
  });

  test("fails closed with no candidates", () => {
    expect(() => trustedGitExecutable([undefined])).toThrow(
      "trusted Nix-store Git executable unavailable",
    );
  });
});

describe("trustedGitExecutable — default discovery never trusts $PATH", () => {
  const env = { ...process.env };
  let scratch: string;

  beforeEach(() => {
    scratch = mkdtempSync(join(tmpdir(), "trusted-git-env-"));
  });
  afterEach(() => {
    process.env = { ...env };
    rmSync(scratch, { recursive: true, force: true });
  });

  test("default resolution ignores a hostile PATH git and yields only a store git", () => {
    // A malicious executable named git, reachable only via PATH.
    const evilDir = join(scratch, "evil");
    mkdirSync(evilDir, { recursive: true });
    const evil = join(evilDir, "git");
    writeFileSync(evil, "#!/bin/sh\necho pwned\n");
    chmodSync(evil, 0o755);
    process.env.PATH = evilDir;
    delete process.env.NORTH_GIT_BIN;

    let resolved: string | undefined;
    try {
      resolved = trustedGitExecutable();
    } catch {
      resolved = undefined; // fail-closed is an acceptable outcome
    }
    // Whatever happens, the ambient PATH binary is never selected.
    expect(resolved).not.toBe(evil);
    if (resolved !== undefined) expect(STORE_GIT.test(resolved)).toBe(true);
  });

  test("default resolution follows the user Nix profile entry hint into the store", () => {
    const git = realStoreGit();
    // A fake HOME whose .nix-profile/bin/git is the only reachable trusted pointer.
    const home = join(scratch, "home");
    const profileBin = join(home, ".nix-profile", "bin");
    mkdirSync(profileBin, { recursive: true });
    symlinkSync(git, join(profileBin, "git"));
    process.env.HOME = home;
    process.env.USER = "no-such-user-xyz"; // /etc per-user pointer absent
    process.env.PATH = "/nonexistent";
    delete process.env.NORTH_GIT_BIN;

    // /run/current-system/sw/bin/git may or may not exist on the host; either way
    // the resolved path is a canonical store git, and never the empty PATH.
    const resolved = trustedGitExecutable();
    expect(STORE_GIT.test(resolved)).toBe(true);
  });
});
