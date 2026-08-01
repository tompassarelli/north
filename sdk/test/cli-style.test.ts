import { expect, test } from "bun:test";
import {
  alignedPairs,
  createCliStyle,
  formatBytes,
  formatTokens,
  percentageGauge,
  simpleTable,
  terminalStyleEnabled,
} from "../src/cli-style";

test("terminal styling precedence is FORCE_COLOR, NO_COLOR, then stdout TTY", () => {
  expect(terminalStyleEnabled({ isTTY: false, env: {} })).toBe(false);
  expect(terminalStyleEnabled({ isTTY: true, env: {} })).toBe(true);
  expect(terminalStyleEnabled({ isTTY: false, env: { FORCE_COLOR: "1", NO_COLOR: "1" } })).toBe(true);
  expect(terminalStyleEnabled({ isTTY: true, env: { FORCE_COLOR: "0" } })).toBe(false);
  expect(terminalStyleEnabled({ isTTY: true, env: { FORCE_COLOR: "false" } })).toBe(false);
  expect(terminalStyleEnabled({ isTTY: true, env: { NO_COLOR: "" } })).toBe(false);
});

test("semantic palette emits ANSI only when terminal styling is enabled", () => {
  const styled = createCliStyle({ isTTY: true, env: {} });
  expect(styled.ok("ready")).toBe("\u001b[32mready\u001b[0m");
  expect(styled.warn("low")).toBe("\u001b[33mlow\u001b[0m");
  expect(styled.crit("spent")).toBe("\u001b[31mspent\u001b[0m");
  expect(styled.accent("account")).toBe("\u001b[36maccount\u001b[0m");
  expect(styled.dim("source")).toBe("\u001b[2msource\u001b[0m");

  const plain = createCliStyle({ isTTY: false, env: {} });
  expect([plain.ok("ready"), plain.warn("low"), plain.crit("spent"), plain.accent("account"), plain.dim("source")])
    .toEqual(["ready", "low", "spent", "account", "source"]);
});

test("gauge is ten cells with a numeric percentage", () => {
  expect(percentageGauge(0)).toBe("░░░░░░░░░░ 0%");
  expect(percentageGauge(-5)).toBe("░░░░░░░░░░ 0%");
  expect(percentageGauge(55)).toBe("▓▓▓▓▓▓░░░░ 55%");
  expect(percentageGauge(100)).toBe("▓▓▓▓▓▓▓▓▓▓ 100%");
  expect(percentageGauge(120)).toBe("▓▓▓▓▓▓▓▓▓▓ 100%");
});

test("byte and token amounts support compact and exact forms", () => {
  expect(formatBytes(7_271_741)).toBe("7.27M");
  expect(formatBytes(7_271_741, { exact: true })).toBe("7,271,741");
  expect(formatTokens(1_234)).toBe("1.23K");
  expect(formatTokens(999)).toBe("999");
});

test("aligned pairs and simple tables use stable column widths", () => {
  expect(alignedPairs([["a", "1"], ["long", "2"]], "  ")).toBe("  a:    1\n  long: 2");
  expect(simpleTable(["name", "state"], [["a", "ok"], ["long", "warn"]], "  ")).toBe(
    "  name  state\n  a     ok\n  long  warn",
  );
});
