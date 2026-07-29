import { expect, test } from "bun:test";
import {
  chmodSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";
import {
  accountAvailabilityBand,
  accountAvailabilityRowIsUsable,
  normalizeAccountAvailability,
} from "../src/account-availability";
import type { ProviderUsageObservationStore } from "../src/providers/types";

const ACCOUNT_PROCESS_TEST_TIMEOUT_MS = 45_000;

interface AvailabilityFixture {
  now: string;
  cases: Array<{
    name: string;
    observation: ProviderUsageObservationStore["observations"][number];
    expected: {
      verdict: string;
      stale: boolean;
      usableModels: string[];
    };
  }>;
}

const fixture = JSON.parse(readFileSync(
  join(import.meta.dir, "fixtures/account-availability.json"),
  "utf8",
)) as AvailabilityFixture;

test("normalizes cached usage evidence across every availability verdict", () => {
  for (const entry of fixture.cases) {
    const [row] = normalizeAccountAvailability(
      { version: 1, observations: [entry.observation] },
      { now: new Date(fixture.now) },
    );
    expect(row, entry.name).toBeDefined();
    expect({
      verdict: row!.verdict,
      stale: row!.stale,
      usableModels: row!.usableModels,
    }, entry.name).toEqual(entry.expected);
  }
});

test("keeps the pinned row shape and provider rung names stable", () => {
  const available = fixture.cases.find(({ name }) => name === "available")!;
  const [row] = normalizeAccountAvailability(
    { version: 1, observations: [available.observation] },
    { now: new Date(fixture.now) },
  );
  expect(row).toEqual({
    account: "claude-available",
    provider: "anthropic",
    observedAt: "2026-07-28T11:00:00.000Z",
    stale: false,
    rungs: {
      window: { name: "five_hour", pct: 20, resetsAt: "2026-07-28T15:00:00.000Z" },
      week: { pct: 30, resetsAt: "2026-08-02T00:00:00.000Z" },
      models: {
        fable: { pct: 40, resetsAt: "2026-08-02T00:00:00.000Z" },
      },
    },
    verdict: "available",
    usableModels: ["fable"],
  });
});

test("maps cached Codex primary evidence into the provider-neutral window rung", () => {
  const available = fixture.cases.find(({ name }) => name === "available-codex")!;
  const [row] = normalizeAccountAvailability(
    { version: 1, observations: [available.observation] },
    { now: new Date(fixture.now) },
  );
  expect(row!.rungs).toEqual({
    window: { name: "primary", pct: 35, resetsAt: "2026-08-02T00:00:00.000Z" },
    week: null,
    models: {},
  });
  expect(accountAvailabilityRowIsUsable(row!)).toBe(true);
});

test("missing provider-required general rungs are explicit unknown capacity", () => {
  const available = fixture.cases.find(({ name }) => name === "available")!.observation;
  const anthropic = {
    ...available,
    targetId: "claude-missing-week",
    windows: available.windows!.filter(({ limitId }) => limitId !== "claude:seven_day"),
  };
  const codex = fixture.cases.find(({ name }) => name === "available-codex")!.observation;
  const openai = {
    ...codex,
    targetId: "codex-missing-primary",
    windows: [],
  };
  const rows = normalizeAccountAvailability({
    version: 1,
    observations: [anthropic, openai],
  }, { now: new Date(fixture.now) });

  expect(rows.map(({ account, verdict, usableModels }) => ({
    account, verdict, usableModels,
  }))).toEqual([
    {
      account: "claude-missing-week",
      verdict: "unknown",
      usableModels: [],
    },
    {
      account: "codex-missing-primary",
      verdict: "unknown",
      usableModels: [],
    },
  ]);
  expect(rows.every((row) => accountAvailabilityRowIsUsable(row) === false)).toBe(true);
});

test("model selection makes a cooked requested model unusable without cooking the account", () => {
  const model = fixture.cases.find(({ name }) => name === "model-cooked")!;
  const store = { version: 1 as const, observations: [model.observation] };
  const fable = normalizeAccountAvailability(store, {
    model: "fable",
    now: new Date(fixture.now),
  });
  const opus = normalizeAccountAvailability(store, {
    model: "claude:model:opus",
    now: new Date(fixture.now),
  });
  expect(fable[0]!.verdict).toBe("model-cooked[fable]");
  expect(accountAvailabilityRowIsUsable(fable[0]!, "fable")).toBe(false);
  expect(opus[0]!.verdict).toBe("available");
  expect(opus[0]!.usableModels).toEqual(["opus"]);
  expect(accountAvailabilityRowIsUsable(opus[0]!, "opus")).toBe(true);
});

test("configurable warning and cooked thresholds remain distinct", () => {
  expect(accountAvailabilityBand(94)).toBe("available");
  expect(accountAvailabilityBand(95)).toBe("warn");
  expect(accountAvailabilityBand(98)).toBe("cooked");
  expect(accountAvailabilityBand(90, { warn: 80, cooked: 90 })).toBe("cooked");
  expect(() => accountAvailabilityBand(90, { warn: 98, cooked: 98 }))
    .toThrow("0 <= warn < cooked <= 100");
});

test("ignores non-authoritative rate-event evidence and filters exact accounts", () => {
  const available = fixture.cases.find(({ name }) => name === "available")!.observation;
  const rows = normalizeAccountAvailability({
    version: 1,
    observations: [
      available,
      {
        targetId: "claude-event",
        provider: "anthropic",
        source: "claude-agent-sdk:rate-limit-event",
        observedAt: fixture.now,
        windows: [{
          limitId: "claude:five_hour",
          usedPercent: 100,
          resetsAt: "2026-07-28T15:00:00.000Z",
        }],
      },
    ],
  }, {
    accounts: [{ id: "claude-available", provider: "anthropic" }],
    now: new Date(fixture.now),
  });
  expect(rows.map(({ account }) => account)).toEqual(["claude-available"]);
});

test("account availability JSON reads only the cached fixture and uses usability for exit status", () => {
  const home = mkdtempSync(join(tmpdir(), "north-availability-cli-"));
  const state = join(home, ".local/state/north");
  const config = join(home, ".config/north");
  const observations = join(state, "provider-usage-observations.json");
  const providerCanary = join(home, "provider-called");
  const providerBinary = join(home, "provider-canary");
  mkdirSync(state, { recursive: true });
  mkdirSync(config, { recursive: true });
  writeFileSync(join(config, "routing-policy.json"), `${JSON.stringify({
    version: 1,
    mode: "preferential",
    providerOrder: ["anthropic", "openai"],
    targetOrder: ["claude-model"],
    targets: [{
      id: "claude-model",
      provider: "anthropic",
      authMode: "isolated",
      profile: "claude-model",
    }],
  })}\n`);
  const model = fixture.cases.find(({ name }) => name === "model-cooked")!;
  const observedAt = new Date().toISOString();
  const observation = { ...model.observation, observedAt };
  writeFileSync(observations, `${JSON.stringify({
    version: 1,
    observations: [observation],
  })}\n`);
  writeFileSync(providerBinary, `#!/bin/sh\ntouch "${providerCanary}"\nexit 1\n`);
  chmodSync(providerBinary, 0o755);
  const env = {
    ...process.env,
    HOME: home,
    NORTH_PROVIDER_OBSERVATIONS: observations,
    NORTH_ROUTING_POLICY: join(config, "routing-policy.json"),
    NORTH_CLAUDE_BIN: providerBinary,
    NORTH_CODEX_BIN: providerBinary,
  };
  const cli = join(import.meta.dir, "../src/account-cli.ts");
  const all = spawnSync("bun", ["run", cli, "availability", "--json"], {
    env,
    encoding: "utf8",
  });
  const fable = spawnSync("bun", [
    "run", cli, "availability", "--model", "fable", "--json",
  ], {
    env,
    encoding: "utf8",
  });

  expect(all.status).toBe(0);
  expect(JSON.parse(all.stdout)).toEqual([{
    account: "claude-model",
    provider: "anthropic",
    observedAt,
    stale: false,
    rungs: {
      window: { name: "five_hour", pct: 20, resetsAt: "2026-07-28T15:00:00.000Z" },
      week: { pct: 20, resetsAt: "2026-08-02T00:00:00.000Z" },
      models: {
        fable: { pct: 99, resetsAt: "2026-08-02T00:00:00.000Z" },
        opus: { pct: 30, resetsAt: "2026-08-02T00:00:00.000Z" },
      },
    },
    verdict: "model-cooked[fable]",
    usableModels: ["opus"],
  }]);
  expect(fable.status).toBe(1);
  expect(JSON.parse(fable.stdout)[0].usableModels).toEqual([]);
  expect(existsSync(providerCanary)).toBe(false);
}, ACCOUNT_PROCESS_TEST_TIMEOUT_MS);
