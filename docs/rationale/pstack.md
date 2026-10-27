# pstack integration rationale

Cold-path reasoning for which pstack practices North adopted, and which it did not (decided 2026-10-15). Agents do
not load this file with rules; each adopted rule lives in the skill, hook or fleet prompt named below.

## Decision

Two independent adversarial reviews, one by Claude Opus 5.5 and one by GPT-6 Astra, compared pstack (Lauren Tan, MIT,
https://github.com/cursor/plugins/tree/main/pstack, read at commit e5136d8) with North's harness. Both found that
pstack has the better engineering process today and North has the better foundation, and both scored North's operator
experience lowest of the dimensions compared (3/10 and 4/10). The decision keeps North and imports the mechanisms
both reviewers held independently, in throughput order. Rules adopted from only one reviewer are marked.

Adopted:

1. Receipts are runtime facts. A worker's last message is its report; fleet records delivery and completion. No
   status files, acknowledgement lines or re-push loops.
2. Verdicts bind to artifacts. Landed means the sha was seen on origin/main. A pass is the named check's exit code,
   captured by fleet, tied to a revision or patch-id and its inputs, and given by someone other than the author. The
   run ledger holds these.
3. Every maintained product has one executed verification driver (launch, drive, observe, clean up), starting with
   `verify-north`.
4. A repeated correction is fixed at the highest level that works: architecture, then types, lint, test, and prose
   last. The fix is proved against a real past mistake, and prose is deleted once an enforcer holds the rule
   ([agent-policy skill](../../harness/skills/agent-policy/SKILL.md)).
5. Prompt and routing changes are promoted only through a blinded A/B on a frozen task set at a fixed model and
   budget, one change at a time, including the fleet system prompt against vanilla Claude Code and Codex.
6. No number is reported without its run count, spread, limiting resource and a paired vanilla comparison.
7. After two failed fixes, the shared assumption is written down and tested before a third fix or a model
   escalation ([debugging skill](../../agent-machinery/skills/debugging/SKILL.md)).
8. The operator surface is derived from state: what changed and which decisions are the owner's. Every question
   carries options, a recommendation, a default and when the default applies, and the agent keeps working.

Adopted from the Opus review only: a flat proxy-to-workers structure with a lead only for a multi-session
workstream; one standing-orders file injected at every fleet launch and revive, gaining a line whenever the owner
repeats an instruction; mechanical bulk work as one script; running something when running it answers the question.
Adopted from the Codex review only: consequential briefs carry the behaviour trace and the rationale behind the
current design.

## Not adopted

- pstack's per-reply ritual and principle-naming.
- Design panels or multi-candidate runs on routine work; the planning skill keeps its consequential-decision trigger.
- Agreement between models as proof of correctness.
- A tool for every task.
- Force-with-lease shipping, and external team actions without asking.
- pstack's file and TSV orchestration store and decision-log files, which would rebuild the message bus North is
  deleting.
- Static model configuration; routing stays measured.
- Seven-source archaeology by default.
- An LLM agent to delete comments; a lint does it.

## Skill consolidation

Both reviews proposed merging the 51 skills (about 25 from Opus, 32 from Codex). The merges both proposed were done
first: prior-art, ceremony-budget, program-stewardship and iteration-economics into planning; testing into
verification; agent-runtime-incident into debugging; skill-maintenance, importing-skills and guard-authoring into
agent-policy; and orchestrating-codex, cloud-workers and auto-wake into workers. Codex stays a first-class provider;
only its terminal-driving procedure went. The result was 27 skills (north 45c7cf0fb, 2126755c9). Four Opus-only merges
were kept because each merged description keeps every old trigger; nix, accounts, game-prototyping and todo stay
unmerged until a probe shows a retrieval miss.

## Alternatives that lost

- Adopting pstack wholesale: it is built for a team on Cursor and Slack, and its orchestration state would re-create
  the bus.
- Leaving North unchanged: both reviewers named the same ceremony costs, and the run ledger had no artifact-bound
  verdicts.

Trade-off accepted: North keeps heavier machinery than pstack for fleet operation, in exchange for owning its prompt,
routing and ledger so improvements can be measured.

Overturned by: the blinded A/B in item 5 showing the vanilla harness matching the fleet prompt on the frozen task
set, or the ceremony in the replaced prompts and watchdogs surviving the rebuild.

Measurement gap (2026-10-15): single-turn policy replay cannot detect the rule to reason rather than reflexively
agree. Four scenarios showed no difference between the full and minus arms and were deleted; the behaviour came from
multi-turn momentum. The fix is a multi-turn replay built from real fleet transcripts.

The 2026-10-27 follow-up review of the same source is [watch/2026-10-27-pstack.md](watch/2026-10-27-pstack.md).

Evidence: [references.md#pstack](../references.md#pstack); the full reviews, per-skill verdict tables and the
decision record are private (south `agents/decisions/harness/rebuild-pstack-*.md`).
