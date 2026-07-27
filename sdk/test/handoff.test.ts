import { describe, expect, test } from "bun:test";
import {
  checkHandoff,
  composeHandoffSpawn,
  fireHandoff,
  parseAvailabilityRows,
  recoveryPinEvidence,
  thresholdCrossings,
  type AvailabilityRow,
} from "../src/handoff";

const reset = "2026-07-29T00:00:00.000Z";
const observedAt = "2026-07-28T05:00:00.000Z";

function row(
  account: string,
  provider: "anthropic" | "openai",
  values: {
    week?: number;
    window?: number;
    models?: Record<string, number>;
    stale?: boolean;
    usableModels?: string[];
    verdict?: string;
  } = {},
): AvailabilityRow {
  const defaultModel = provider === "anthropic" ? "claude-opus-4-1" : "gpt-5.6-sol";
  return {
    account,
    provider,
    observedAt,
    stale: values.stale ?? false,
    rungs: {
      window: { name: "five-hour", pct: values.window ?? 20, resetsAt: reset },
      week: { pct: values.week ?? 20, resetsAt: reset },
      models: Object.fromEntries(
        Object.entries(values.models ?? { [defaultModel]: 20 })
          .map(([model, pct]) => [model, { pct, resetsAt: reset }]),
      ),
    },
    verdict: values.verdict ?? "available",
    usableModels: values.usableModels ?? [defaultModel],
  };
}

const openaiHeir = row("codex-heir", "openai", {
  usableModels: ["gpt-5.6-sol"],
});

describe("active route classification and same-strength heir selection", () => {
  test("week threshold kills the account", () => {
    const check = checkHandoff([
      row("claude-active", "anthropic", { week: 80, verdict: "cooked-week" }),
      openaiHeir,
    ], {
      provider: "anthropic",
      account: "claude-active",
      model: "claude-opus-4-1",
      tier: "senior",
    }, 80);
    expect(check.classification).toBe("account-dead");
    expect(check.trigger?.rung).toBe("week");
    expect(check.heir).toEqual({
      provider: "openai",
      account: "codex-heir",
      model: "gpt-5.6-sol",
      tier: "senior",
      observedAt,
    });
  });

  test("window threshold kills the route until reset", () => {
    const check = checkHandoff([
      row("claude-active", "anthropic", { window: 91, verdict: "cooked-window" }),
      openaiHeir,
    ], {
      provider: "anthropic", account: "claude-active",
      model: "claude-opus-4-1", tier: "senior",
    }, 80);
    expect(check.classification).toBe("window-dead");
    expect(check.trigger).toMatchObject({ rung: "window", resetsAt: reset });
  });

  test("model threshold keeps the account alive and kills only the active model", () => {
    const check = checkHandoff([
      row("claude-active", "anthropic", {
        models: { "claude-opus-4-1": 95, "claude-sonnet-4-5": 10 },
        usableModels: ["claude-sonnet-4-5"],
        verdict: "model-cooked[claude-opus-4-1]",
      }),
      openaiHeir,
    ], {
      provider: "anthropic", account: "claude-active",
      model: "claude-opus-4-1", tier: "senior",
    }, 80);
    expect(check.classification).toBe("model-dead");
    expect(check.trigger).toMatchObject({ rung: "model", model: "claude-opus-4-1" });
    expect(check.receipts.active.rungs.week.pct).toBe(20);
  });

  test("below-threshold route stays active and does not name an heir", () => {
    const check = checkHandoff([
      row("claude-active", "anthropic"),
      openaiHeir,
    ], {
      provider: "anthropic", account: "claude-active",
      model: "claude-opus-4-1", tier: "senior",
    }, 80);
    expect(check.classification).toBe("available");
    expect(check.heir).toBeUndefined();
  });

  test("stale and model-cooked candidates do not count as capacity", () => {
    const check = checkHandoff([
      row("claude-active", "anthropic", { week: 100, verdict: "cooked-week" }),
      row("codex-stale", "openai", { stale: true }),
      row("codex-cooked", "openai", {
        models: { "gpt-5.6-sol": 90 },
        usableModels: ["gpt-5.6-terra"],
        verdict: "model-cooked[gpt-5.6-sol]",
      }),
    ], {
      provider: "anthropic", account: "claude-active",
      model: "claude-opus-4-1", tier: "senior",
    }, 80);
    expect(check.heir).toBeUndefined();
  });
});

test("pinned availability parser rejects shape drift", () => {
  const valid = row("codex-heir", "openai");
  expect(parseAvailabilityRows([valid])).toEqual([valid]);
  expect(() => parseAvailabilityRows([{ ...valid, surprise: true }]))
    .toThrow("fields mismatch");
});

test("warning detection reports every crossed rung", () => {
  const crossings = thresholdCrossings(row("claude-active", "anthropic", {
    week: 82,
    window: 81,
    models: { "claude-opus-4-1": 95, "claude-sonnet-4-5": 20 },
  }), 80);
  expect(crossings.map(({ rung, name }) => `${rung}:${name}`)).toEqual([
    "week:week",
    "window:five-hour",
    "model:claude-opus-4-1",
  ]);
});

test("provider-recovery evidence pins the complete heir route and embeds receipts", () => {
  const check = checkHandoff([
    row("claude-active", "anthropic", { week: 100, verdict: "cooked-week" }),
    openaiHeir,
  ], {
    provider: "anthropic", account: "claude-active",
    model: "claude-opus-4-1", tier: "senior",
  }, 80);
  const evidence = recoveryPinEvidence(check, new Date("2026-07-28T05:30:00.000Z"));
  expect(evidence.reasonCode).toBe("provider-recovery");
  expect(evidence.pins).toEqual([
    { kind: "provider", value: "openai" },
    { kind: "account", value: "codex-heir" },
    { kind: "model", value: "gpt-5.6-sol" },
  ]);
  expect(evidence.detail).toContain('"activeReceipt"');
  expect(evidence.detail).toContain('"heirReceipt"');
});

test("dry-run composition is complete and execution remains injectable", () => {
  const check = checkHandoff([
    row("claude-active", "anthropic", { window: 100, verdict: "cooked-window" }),
    openaiHeir,
  ], {
    provider: "anthropic", account: "claude-active",
    model: "claude-opus-4-1", tier: "senior",
  }, 80);
  const facts = new Map([
    ["root", [{ predicate: "title", value: "root program" }]],
    ["child", [{ predicate: "title", value: "child lane" }, { predicate: "part_of", value: "@root" }]],
  ]);
  const spawn = composeHandoffSpawn(check, "root", "/fixture/succession.md", "human", {
    now: new Date("2026-07-28T05:30:00.000Z"),
    northBin: "/fixture/north",
    peerBb: "/fixture/bb",
    msgCli: "/fixture/msg-cli.clj",
    env: { AGENT_ID: "coordinator", NORTH_PORT: "9000" },
    readBrief: () => "succession context",
    getChildren: () => ["child"],
    getFacts: (id) => facts.get(id) ?? [],
  });
  expect(spawn.command.args.slice(0, 2)).toEqual(["spawn", "team-lead"]);
  expect(spawn.command.args).toContain("--thread");
  expect(spawn.command.args).toContain("--provider");
  expect(spawn.command.args).toContain("--target");
  expect(spawn.command.args).toContain("--model");
  expect(spawn.command.args).toContain("--pin-evidence");
  expect(spawn.command.args).toContain("--notify");
  expect(spawn.context.brief.content).toBe("succession context");
  expect(spawn.context.threadMap.map(({ id }) => id)).toEqual(["root", "child"]);
  expect(spawn.prompt).toContain("THREAD MAP");
  expect(spawn.notification.args).toEqual([
    "/fixture/msg-cli.clj", "9000", "send", "coordinator", "human",
    "PROVIDER HANDOFF FIRED",
    "@root -> openai/codex-heir/gpt-5.6-sol (senior); reason=provider-recovery",
  ]);

  const commands: Array<[string, string[]]> = [];
  fireHandoff(spawn, {
    run: (executable, args) => {
      commands.push([executable, args]);
      return { status: 0 };
    },
  });
  expect(commands).toEqual([
    [spawn.command.executable, spawn.command.args],
    [spawn.notification.executable, spawn.notification.args],
  ]);
});

test("available routes and missing heirs refuse fire composition", () => {
  const active = {
    provider: "anthropic" as const,
    account: "claude-active",
    model: "claude-opus-4-1",
    tier: "senior" as const,
  };
  const runtime = {
    readBrief: () => "brief",
    getChildren: () => [],
    getFacts: () => [],
  };
  expect(() => composeHandoffSpawn(
    checkHandoff([row("claude-active", "anthropic")], active, 80),
    "root", "/brief", "human", runtime,
  )).toThrow("has not crossed");
  expect(() => composeHandoffSpawn(
    checkHandoff([
      row("claude-active", "anthropic", { week: 100, verdict: "cooked-week" }),
    ], active, 80),
    "root", "/brief", "human", runtime,
  )).toThrow("no same-tier");
});
