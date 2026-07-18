import { execFileSync } from "node:child_process";

export interface Fact {
  predicate: string;
  value: string;
}

const NORTH_ENTITY_ID = /^[a-z0-9][a-z0-9._:-]*$/i;

export class InvalidNorthEntityIdError extends Error {
  readonly code = "NORTH_INVALID_ENTITY_ID";
  readonly preSideEffect = true;

  constructor() {
    super("invalid North entity id: expected a bare or single-@ ASCII identifier");
    this.name = "InvalidNorthEntityIdError";
  }
}

export class NorthReadUnavailableError extends Error {
  readonly code = "NORTH_READ_UNAVAILABLE";
  readonly preSideEffect = true;

  constructor(readonly operation: "facts" | "children") {
    super(`North ${operation} read is unavailable`);
    this.name = "NorthReadUnavailableError";
  }
}

export class NorthReadProtocolError extends Error {
  readonly code = "NORTH_READ_PROTOCOL_ERROR";
  readonly preSideEffect = true;

  constructor(readonly operation: "facts" | "children") {
    super(`North ${operation} read returned an invalid response`);
    this.name = "NorthReadProtocolError";
  }
}

export interface NorthReadOptions {
  command?: string;
  timeoutMs?: number;
}

/** Canonical SDK form is bare; graph operations add the single `@` sigil. */
export function normalizeNorthEntityId(input: string): string {
  if (typeof input !== "string" || input !== input.trim()) {
    throw new InvalidNorthEntityIdError();
  }
  const bare = input.startsWith("@") ? input.slice(1) : input;
  if (!bare || bare.length > 512 || bare.startsWith("@") || !NORTH_ENTITY_ID.test(bare)) {
    throw new InvalidNorthEntityIdError();
  }
  return bare;
}

export function northEntitySubject(input: string): string {
  return `@${normalizeNorthEntityId(input)}`;
}

function invokeNorth(
  operation: "facts" | "children",
  args: string[],
  options: NorthReadOptions,
): string {
  try {
    return execFileSync(options.command ?? process.env.NORTH_BIN ?? "north", args, {
      encoding: "utf-8",
      timeout: options.timeoutMs ?? 5000,
      stdio: ["ignore", "pipe", "pipe"],
    });
  } catch {
    throw new NorthReadUnavailableError(operation);
  }
}

export function getThreadFacts(
  threadId: string,
  options: NorthReadOptions = {},
): Fact[] {
  const canonicalId = normalizeNorthEntityId(threadId);
  const out = invokeNorth("facts", ["json", "show", canonicalId], options);
  let parsed: unknown;
  try {
    parsed = JSON.parse(out.trim());
  } catch {
    throw new NorthReadProtocolError("facts");
  }
  if (!Array.isArray(parsed) || !parsed.every((fact) =>
    fact !== null
    && typeof fact === "object"
    && typeof (fact as Record<string, unknown>).predicate === "string"
    && typeof (fact as Record<string, unknown>).value === "string")) {
    throw new NorthReadProtocolError("facts");
  }
  return parsed as Fact[];
}

export function getChildren(
  parentId: string,
  options: NorthReadOptions = {},
): string[] {
  const canonicalId = normalizeNorthEntityId(parentId);
  const query = `{:find "child" :rules [{:head {:rel "child" :args [{:var "child"}]} :body [{:rel "triple" :args [{:var "child"} "part_of" "@${canonicalId}"]}]}]}`;
  const out = invokeNorth("children", ["query", query], options);
  const lines = out.split("\n").map((value) => value.trim()).filter(Boolean);
  if (lines.length === 0 || (lines.length === 1 && lines[0] === "(no results)")) {
    return [];
  }
  const children: string[] = [];
  for (const line of lines) {
    let row: unknown;
    try {
      row = JSON.parse(line);
    } catch {
      throw new NorthReadProtocolError("children");
    }
    if (!Array.isArray(row) || row.length !== 1 || typeof row[0] !== "string"
      || !row[0].startsWith("@") || normalizeNorthEntityId(row[0]) !== row[0].slice(1)) {
      throw new NorthReadProtocolError("children");
    }
    children.push(row[0].slice(1));
  }
  return children;
}
