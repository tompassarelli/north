const DEFAULT_MAX_BYTES = 1024 * 1024;
const DEFAULT_MAX_DEPTH = 128;
const DEFAULT_MAX_NODES = 100_000;

export interface StrictJsonLimits {
  maxBytes?: number;
  maxDepth?: number;
  maxNodes?: number;
}

export function assertWellFormedUnicode(value: string, label: string): void {
  for (let index = 0; index < value.length; index++) {
    const unit = value.charCodeAt(index);
    if (unit >= 0xd800 && unit <= 0xdbff) {
      const next = value.charCodeAt(index + 1);
      if (!(next >= 0xdc00 && next <= 0xdfff))
        throw new Error(`${label} contains ill-formed Unicode`);
      index++;
    } else if (unit >= 0xdc00 && unit <= 0xdfff) {
      throw new Error(`${label} contains ill-formed Unicode`);
    }
  }
}

function inspectParsedValue(
  value: unknown,
  label: string,
  maxDepth: number,
  maxNodes: number,
): void {
  let nodes = 0;
  const visit = (current: unknown, depth: number): void => {
    if (++nodes > maxNodes) throw new Error(`${label} exceeds the bounded JSON node count`);
    if (depth > maxDepth) throw new Error(`${label} exceeds the bounded JSON nesting depth`);
    if (typeof current === "string") {
      assertWellFormedUnicode(current, label);
      return;
    }
    if (Array.isArray(current)) {
      for (const entry of current) visit(entry, depth + 1);
      return;
    }
    if (typeof current === "object" && current !== null) {
      for (const [key, entry] of Object.entries(current as Record<string, unknown>)) {
        assertWellFormedUnicode(key, label);
        visit(entry, depth + 1);
      }
    }
  };
  visit(value, 0);
}

/**
 * Parse one bounded JSON document while rejecting duplicate object members.
 * JSON.parse alone is deliberately insufficient: it silently applies a
 * last-member-wins policy that can change an authority-bearing envelope.
 */
export function parseStrictJson(
  text: string,
  label: string,
  limits: StrictJsonLimits = {},
): unknown {
  assertWellFormedUnicode(text, label);
  const maxBytes = limits.maxBytes ?? DEFAULT_MAX_BYTES;
  const maxDepth = limits.maxDepth ?? DEFAULT_MAX_DEPTH;
  const maxNodes = limits.maxNodes ?? DEFAULT_MAX_NODES;
  if (Buffer.byteLength(text, "utf8") > maxBytes)
    throw new Error(`${label} exceeds the bounded JSON byte size`);

  const stack: Array<{ kind: "object"; keys: Set<string> } | { kind: "array" }> = [];
  for (let index = 0; index < text.length; index++) {
    const character = text[index]!;
    if (character === "\"") {
      const start = index;
      let escaped = false;
      for (index++; index < text.length; index++) {
        const stringCharacter = text[index]!;
        if (escaped) escaped = false;
        else if (stringCharacter === "\\") escaped = true;
        else if (stringCharacter === "\"") break;
      }
      if (index >= text.length) throw new Error(`${label} is invalid JSON`);
      let next = index + 1;
      while (/[ \t\r\n]/.test(text[next] ?? "")) next++;
      const context = stack.at(-1);
      if (text[next] === ":" && context?.kind === "object") {
        let key: unknown;
        try { key = JSON.parse(text.slice(start, index + 1)); }
        catch { throw new Error(`${label} is invalid JSON`); }
        if (typeof key !== "string") throw new Error(`${label} is invalid JSON`);
        if (context.keys.has(key)) throw new Error(`${label} contains duplicate object keys`);
        context.keys.add(key);
      }
    } else if (character === "{") {
      stack.push({ kind: "object", keys: new Set() });
      if (stack.length > maxDepth) throw new Error(`${label} exceeds the bounded JSON nesting depth`);
    } else if (character === "[") {
      stack.push({ kind: "array" });
      if (stack.length > maxDepth) throw new Error(`${label} exceeds the bounded JSON nesting depth`);
    } else if (character === "}" || character === "]") {
      stack.pop();
    }
  }

  let parsed: unknown;
  try { parsed = JSON.parse(text) as unknown; }
  catch { throw new Error(`${label} is invalid JSON`); }
  inspectParsedValue(parsed, label, maxDepth, maxNodes);
  return parsed;
}
