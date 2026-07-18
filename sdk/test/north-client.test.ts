import { afterEach, expect, test } from "bun:test";
import { chmodSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import {
  getChildren,
  getThreadFacts,
  InvalidNorthEntityIdError,
  NorthReadProtocolError,
  NorthReadUnavailableError,
  normalizeNorthEntityId,
  northEntitySubject,
} from "../src/north-client";

const temporary: string[] = [];
afterEach(() => {
  for (const path of temporary.splice(0)) rmSync(path, { recursive: true, force: true });
});

function fakeNorth(body: string): string {
  const directory = mkdtempSync(join(tmpdir(), "north-client-"));
  temporary.push(directory);
  const command = join(directory, "north");
  writeFileSync(command, `#!/usr/bin/env bash\n${body}\n`);
  chmodSync(command, 0o755);
  return command;
}

test("North entity ids have one canonical bare and graph-subject form", () => {
  const id = "019f75a8-032c-741a-b65d-e4af097e3837";
  expect(normalizeNorthEntityId(id)).toBe(id);
  expect(normalizeNorthEntityId(`@${id}`)).toBe(id);
  expect(northEntitySubject(id)).toBe(`@${id}`);
  expect(northEntitySubject(`@${id}`)).toBe(`@${id}`);
});

test("North entity ids reject ambiguity and injection shapes before reads", () => {
  for (const invalid of [
    "", "@", "@@019f75a8-032c-741a-b65d-e4af097e3837",
    " 019f75a8-032c-741a-b65d-e4af097e3837",
    "019f75a8-032c-741a-b65d-e4af097e3837 ",
    "thread;touch-owned", "thread$(touch-owned)", "thread\nnext", "thread/@other",
  ]) {
    let error: unknown;
    try { normalizeNorthEntityId(invalid); } catch (caught) { error = caught; }
    expect(error).toBeInstanceOf(InvalidNorthEntityIdError);
    expect(error).toMatchObject({
      code: "NORTH_INVALID_ENTITY_ID",
      preSideEffect: true,
    });
  }
});

test("children use the supported structured query and distinguish parent from leaf", () => {
  const command = fakeNorth(`
if [ "$1" != query ]; then exit 91; fi
case "$2" in
  *019f75a8-032c-741a-b65d-e4af097e3837*)
    printf '  ["@019f75a8-032c-741a-b65d-e4af097e3838"]\\n'
    printf '  ["@019f75a8-032c-741a-b65d-e4af097e3839"]\\n'
    ;;
esac
`);
  expect(getChildren("019f75a8-032c-741a-b65d-e4af097e3837", { command })).toEqual([
    "019f75a8-032c-741a-b65d-e4af097e3838",
    "019f75a8-032c-741a-b65d-e4af097e3839",
  ]);
  expect(getChildren("019f75a8-032c-741a-b65d-e4af097e3840", { command })).toEqual([]);

  const explicitLeaf = fakeNorth("printf '  (no results)\\n'");
  expect(getChildren("019f75a8-032c-741a-b65d-e4af097e3840", { command: explicitLeaf }))
    .toEqual([]);
});

test("North reads distinguish authoritative absence from transport and protocol failure", () => {
  const absent = fakeNorth("printf '[]\\n'");
  expect(getThreadFacts("019f75a8-032c-741a-b65d-e4af097e3837", { command: absent }))
    .toEqual([]);

  const malformed = fakeNorth("printf 'usage: json board|ready|show\\n'");
  expect(() => getThreadFacts(
    "019f75a8-032c-741a-b65d-e4af097e3837", { command: malformed },
  )).toThrow(NorthReadProtocolError);
  expect(() => getChildren(
    "019f75a8-032c-741a-b65d-e4af097e3837", { command: malformed },
  )).toThrow(NorthReadProtocolError);

  const unavailable = fakeNorth("exit 17");
  expect(() => getThreadFacts(
    "019f75a8-032c-741a-b65d-e4af097e3837", { command: unavailable },
  )).toThrow(NorthReadUnavailableError);
  expect(() => getChildren(
    "019f75a8-032c-741a-b65d-e4af097e3837", { command: unavailable },
  )).toThrow(NorthReadUnavailableError);
});
