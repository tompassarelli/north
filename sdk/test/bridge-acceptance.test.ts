import { expect, test } from "bun:test";
import { runBridgeAcceptance } from "../src/bridge/accept";

test("Stage 3 acceptance survives UI death and explains two controlled sessions from journals", async () => {
  const output: string[] = [];
  const result = await runBridgeAcceptance({ output: (line) => output.push(line) });

  expect(result).toEqual(output);
  expect(output.filter((line) => line.startsWith("PASS "))).toHaveLength(8);
  expect(output.at(-1)).toBe("ACCEPTANCE PASS 8/8");
  expect(output).not.toContainEqual(expect.stringMatching(/^FAIL /));
});
