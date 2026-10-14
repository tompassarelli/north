# Lead brief template

Start a lead with `agents lead start --provider claude|codex --domain D --brief ~/.local/state/agents/handoffs/<name>-brief.md`.
The launcher registers it in `agents org`, sets AGENT_ROLE, AGENT_DEPTH,
AGENT_DELEGATION_BUDGET and AGENT_ORG_NAME, and keeps `<name>-status.md` beside
the brief as its status file. A sub-lead is the same command with
`--role sub-lead`, run from the lead's session.

```
Delegation: role=lead budget=2
# <Domain> lead brief (<date>)

Goal: <Tom's goal for the domain, and what finished means>.
Read <repo>/AGENTS.md first. Resume state: <status file>.

Loop:
1. For each Done when line you write, name the existing property, replay or
   golden test that covers it; a new example test needs a one-line reason.
   Schedule your tick, staff every unblocked box up to the spawn gate, land
   what passes, and append one status line per box to <status file>. Start a
   fix the moment you find it; a review or report your parent names is a
   report point, not a start gate, unless the fix depends on what is under review.
2. Layering: with 2 or more independent workstreams, or more than 6 workers in
   flight, start a sub-lead without asking (`agents lead start --role sub-lead
   --domain D --brief FILE --budget 1`). Its budget must stay below yours. It
   merges back and deregisters when its workstream finishes.
3. Briefs you write begin with `Delegation: role=<worker|sub-lead> budget=<n>`,
   n below your own; a brief without it is a worker with budget 0. The budget
   limits how many levels of delegation may follow below you, not how many
   workers you run: the spawn gate and ready work bound the count, so staff
   every unblocked box. Give a big or uncalibrated item budget 1, so its
   worker can report `Split:` and lead the parts itself (workers skill). Take each brief's ETA from the `eta` line of `worker-ledger --recommend`, marking an estimate `(uncalibrated)` when it prints `eta n/a`. Research briefs are the exception: they are timeboxed at 5 minutes and end by offering a deeper pass with its own ETA, rather than starting one.
4. Capacity: when the spawn gate refuses you on domain priority, move code-only
   work to a cloud worker or queue it. Take a real conflict to the proxy.
5. Cross-project: when your work touches another lead's, message that lead
   directly at the `message uds:` address `agents org show` prints for it,
   and copy the proxy only on a conflict. Turn each problem you find that spans projects into one automatic
   flag in `agents flow` or capacity-watchdog that you own, so every tick sees
   it instead of a person noticing it again.

Covered harness changes land only with a `Reviewed:` trailer from a fresh reviewer who did not author the change: Astra high/xhigh, Sol high/xhigh or Opus. This is the interim review rule until north#52e mechanically enforces review of the combined patch-id. Follow the `workers` skill for the covered paths.

Follow the workers skill's Reports and ticks rule before calling a fix landed; a green or queued Autoland run is a pending landing.

Questions: publish `agents event QUESTION <item> --from-node <your org node> --to-node <parent node> --body '<question>'`; it retains the human record and the native supervisor delivers a turn and escalates unanswered questions. Answer with `agents event ANSWER --from-node <your org node> --to-node <sender> --answer <event id> --body '<answer>'`. Ask only for an escalation item below, or a conflict over another lead's domain or the shared budget. Decide anything inside your brief yourself, including reordering, pulling deferred items forward, reusing valid evidence and choosing between your own boxes; record it as `DECIDED <HH:MM> <item>: <choice and why>`.

After a fresh review, publish `agents event LAND-READY <item> --from-node <your org node> --to-node <parent node> --lane <absolute lane> --sha <reviewed head> --repo <repo> --body '<receipt>'`. Land your own ready work through safe-push. For recycling, settle and archive children and write the restart packet first, then publish `agents event RECYCLE-READY --from-node <your org node> --to-node <parent node> --packet <absolute packet> --body 'children settled; ready to recycle'`. The native supervisor owns the 15-minute restart deadline; a status line alone is not a delivery.

Escalation: only money, accounts, irreversible deletion, choosing between
products, and reversing a direction Tom stated go to Tom, through the proxy.
Decide everything else yourself or with an Opus planner, and show it in
the status file.

Finish: a `DONE <task>` status line marks one task and finishes nothing. When
the workstream's boxes pass and land, append `FINISHED $AGENT_ORG_NAME` as the
last line of <status file> (your parent's tick finishes the node on exactly
that line), run `agents org remove $AGENT_ORG_NAME` and end the session.
```
