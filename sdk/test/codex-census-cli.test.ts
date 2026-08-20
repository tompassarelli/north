import { expect, test } from "bun:test";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { configuredCodexSubscriptionAccounts } from "../src/codex-census-cli";

test("Codex census includes every configured isolated subscription account and no other provider", () => {
  const home = mkdtempSync(join(tmpdir(), "north-codex-census-"));
  const policy = join(home, "routing.json");
  writeFileSync(policy, JSON.stringify({ targets: [
    { id: "codex-work", provider: "openai", profile: "codex-work", authMode: "isolated" },
    { id: "claude-work", provider: "anthropic", profile: "claude-work", authMode: "isolated" },
    { id: "codex-personal", provider: "openai", profile: "codex-personal", authMode: "isolated" },
  ] }));
  expect(configuredCodexSubscriptionAccounts({ home, routingPolicyPath: policy }))
    .toEqual(["codex-personal", "codex-work"]);
});
