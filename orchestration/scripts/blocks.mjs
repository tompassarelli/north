// Shared prompt-block extraction praxis. Both the agent compiler
// (build-agents.mjs) and the payload composer (compose-payload.mjs) consume
// blocks this way; a single extractor keeps the praxis from drifting.

// heading -> first fenced block after it
export function block(text, heading) {
  const lines = text.split("\n");
  const h = `## ${heading.toLowerCase()}`;
  let at = lines.findIndex((l) => l.trim().toLowerCase() === h);
  if (at === -1) throw new Error(`heading not found: ${heading}`);
  let open = -1;
  for (let i = at + 1; i < lines.length; i++) {
    const t = lines[i].trim();
    if (open === -1 && t.startsWith("## ")) break;
    if (open === -1 && t.startsWith("```")) { open = i + 1; continue; }
    if (open !== -1 && t.startsWith("```")) return lines.slice(open, i).join("\n");
  }
  throw new Error(`no fence under heading: ${heading}`);
}

export function firstFence(text) {
  const m = text.match(/```\n([\s\S]*?)\n```/);
  if (!m) throw new Error("no fence in delta doc");
  return m[1];
}
