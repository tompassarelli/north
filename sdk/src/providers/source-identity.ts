import { spawnSync } from "node:child_process";

interface SourceIdentityEnvironment {
  [name: string]: string | undefined;
  NORTH_PACKAGE_MODE?: string;
  NORTH_PACKAGE_REV?: string;
}

/** Human-visible identity for a live checkout, including uncommitted state. */
export function checkoutSourceIdentity(root: string): string {
  const revision = spawnSync("git", ["-C", root, "rev-parse", "--short", "HEAD"],
    { encoding: "utf8", timeout: 1000 });
  if (revision.status !== 0) return "checkout unknown";
  const status = spawnSync("git", ["-C", root, "status", "--porcelain", "--untracked-files=normal"],
    { encoding: "utf8", timeout: 1000 });
  const dirty = status.status !== 0 || status.stdout.trim().length > 0;
  return `checkout ${revision.stdout.trim()}${dirty ? " dirty" : " clean"}`;
}

export function northSourceIdentity(
  root: string,
  environment: SourceIdentityEnvironment = process.env,
): string {
  const revision = environment.NORTH_PACKAGE_REV;
  if (environment.NORTH_PACKAGE_MODE === "nix-store")
    return `nix-store ${revision || "unknown"}`;
  return revision ? `checkout ${revision}` : checkoutSourceIdentity(root);
}
