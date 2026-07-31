#!/usr/bin/env node
// Renders the public agents repository from North-owned canonical sources.
// emit-artifacts.mjs --out <directory> [--check]
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { block, firstFence } from "./blocks.mjs";
import { CONFORMANCE_HEADERS } from "./compose-payload.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const read = (path) => readFileSync(resolve(ROOT, path), "utf8");

function parseArgs(argv) {
  let out;
  let check = false;
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === "--out") {
      out = argv[++i];
      if (!out) throw new Error("missing value for --out");
    } else if (argv[i] === "--check") {
      check = true;
    } else {
      throw new Error(`unknown argument: ${argv[i]}`);
    }
  }
  if (!out) throw new Error("usage: emit-artifacts.mjs --out <directory> [--check]");
  return { out: resolve(process.cwd(), out), check };
}

const family = firstFence(read("docs/deltas/openai-common.md"));
const comms = block(read("docs/comms.md"), "universal");

function canonicalArtifact(name) {
  const source = firstFence(read(`docs/artifacts/${name}.md`));
  const expanded = source.replaceAll("{{family}}", family).replaceAll("{{comms}}", comms);
  const placeholder = expanded.match(/{{[^}]+}}/);
  if (placeholder) throw new Error(`${name}: unknown placeholder ${placeholder[0]}`);
  return expanded.replace(/\n*$/, "\n");
}

function requiredArtifact(name) {
  const content = canonicalArtifact(name);
  const headingEnd = content.indexOf("\n");
  if (headingEnd === -1) throw new Error(`${name}: artifact has no title line`);
  return `${content.slice(0, headingEnd)}\n\n${CONFORMANCE_HEADERS.required}\n${content.slice(headingEnd + 1).replace(/^\n/, "\n")}`;
}

function readme() {
  return `# agents — behavioral protocols for AI coding agents

Drop-in, evidence-backed policy blocks that make GPT-family coding agents
reliable under autonomy: bounded scope, bounded verification, honest
endings, and authority they don't exceed. Every rule traces to an observed
failure in real agent transcripts — none of it is speculation.

## The two artifacts, and what they are

**\`gpt-conduct-protocol/\`** — a *behavior standard*: the general operating
contract for a GPT-family coding agent. It was calibrated by diffing
GPT-5.6-family behavior against a frontier reference model on identical
real-world tasks — but the reference was the *measuring instrument, not the
style target*. Rules correct observed defects (scope inflation, plan
restructuring, silent authority overreach, status-update endings); where
the GPT side beat the reference in calibration (test-first discipline,
hermetic isolation hygiene, evidence custody), its behavior was kept.

**\`gpt-verification-loop-protocol/\`** — a *focused behavior modifier* for
one specific pathology: the verification loop. Endless re-checking,
invented verification methods when a tool breaks, scope that grows
mid-task, "one more pass" that never ships. Formerly published standalone
as **Stop the Loop**; this is its successor, refined against a much larger
evidence base. The fix is not "verify less" — it is claim contracts
declared at intake, terminal states per pass, and named tarpits.

They compose: conduct is the general contract, verification-loop is the
deep dive on one axis. The conduct protocol's verification-budget rule is
the light version of the loop protocol; installing both is coherent and
recommended for autonomous work.

## Steering and conformance

The axes are orthogonal. **Steering** selects how much protocol content
rides in the prompt:

- **\`light.md\`** — the minimum dose; the sharpest edges only.
- **\`moderate.md\`** — the working default.
- **\`strong.md\`** — the full stack for autonomous, long-running, or
  production-adjacent work.

Dial steering up with autonomy, blast radius, and irreversibility; dial it
down when a human reviews each step or when native behavior is the asset.
At every steering level the verification protocol kills the *loop
pathology*, never verification quality itself.

**Conformance** selects only the binding register; it does not add or remove
rules. The exact headers are:

- advisory: \`${CONFORMANCE_HEADERS.advisory}\`
- preferred: \`${CONFORMANCE_HEADERS.preferred}\`
- required: \`${CONFORMANCE_HEADERS.required}\`

The published files use required conformance. A controlled A/B (2026-07)
found that advisory wording preserved every targeted safety behavior and
shortened output, while omitting content reopened loops. Choose steering
and conformance independently and deliberately.

## Install

- **Codex CLI / anything reading \`AGENTS.md\`:** paste the chosen files
  into your global or per-repo \`AGENTS.md\`. The repo-root \`AGENTS.md\` is
  the ready-made default profile (\`gpt-conduct-protocol/moderate.md\` +
  \`gpt-verification-loop-protocol/moderate.md\`, required conformance).
- **API / agent frameworks:** paste them into the system or developer message.
- **Spawning setups:** read the propagation section — children dispatched
  outside the repo root inherit nothing unless the protocol rides in
  their prompt.

Use the blocks content-complete. Choose steering for content amount and
conformance for register; do not approximate either by deleting rules.

## Provenance and method

Distilled 2026-07 from: a 64-session mined corpus of real agent
transcripts across two providers (30 flagged incidents, quotes verified
against raw logs); a 10-item identical-prompt planning harness between a
frontier reference model and gpt-5.6-sol at maximum reasoning; and a
controlled retest in which the composed protocol moved 3 of 3 targeted
safety behaviors on the exact prompts where drift was observed (silent
constraint override → explicit conflict escalation; deliverable descoping
→ engagement with the hard reading; and 33–45% shorter output on 2 of 3).
The verification-loop protocol was additionally adversarially consolidated
between a Claude supervisor and an OpenAI supervisor agent — the OpenAI
side's self-diagnosed failure modes are folded in.

Written for GPT-family agents because that is where the calibration ran;
most rules are provider-neutral. Refinement is ongoing from field data —
issues and PRs with new observed failure modes are welcome.

MIT licensed.
`;
}

function agents() {
  const conduct = canonicalArtifact("conduct-moderate");
  const verification = canonicalArtifact("verification-loop-moderate");
  const conductAt = conduct.indexOf("## Family protocol");
  const verificationAt = verification.indexOf("**Verification policy — overrides your defaults.**");
  if (conductAt === -1 || verificationAt === -1)
    throw new Error("default profile boundaries missing from moderate artifacts");
  return `# AGENTS.md — default profile for GPT-family coding agents

The ready-made default: conduct protocol (moderate steering) +
verification-loop protocol (moderate steering), at required conformance.
Choose either axis per task using README.md. Keep the selected content
complete; omitting rules re-opens the loops they close.

${CONFORMANCE_HEADERS.required}

${conduct.slice(conductAt).trimEnd()}


${verification.slice(verificationAt).trimEnd()}
`;
}

const ARTIFACTS = {
  "README.md": readme(),
  "AGENTS.md": agents(),
  "LICENSE": read("LICENSE-MIT"),
  "gpt-conduct-protocol/light.md": requiredArtifact("conduct-light"),
  "gpt-conduct-protocol/moderate.md": requiredArtifact("conduct-moderate"),
  "gpt-conduct-protocol/strong.md": requiredArtifact("conduct-strong"),
  "gpt-verification-loop-protocol/light.md": requiredArtifact("verification-loop-light"),
  "gpt-verification-loop-protocol/moderate.md": requiredArtifact("verification-loop-moderate"),
  "gpt-verification-loop-protocol/strong.md": requiredArtifact("verification-loop-strong"),
};

const { out, check } = parseArgs(process.argv.slice(2));
let dirty = 0;
for (const [relative, rendered] of Object.entries(ARTIFACTS)) {
  const path = resolve(out, relative);
  const current = existsSync(path) ? readFileSync(path, "utf8") : "";
  if (current === rendered) continue;
  if (check) {
    console.error(`STALE: ${relative}`);
    dirty++;
  } else {
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, rendered);
    console.log(`wrote ${relative}`);
  }
}

if (check && dirty) process.exit(1);
console.log(check ? "check: all current" : "emit: done");
