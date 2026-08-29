# Coordination module

Coordination is North's shared-ledger and operational agent-run module. It has
two members:

| Member | Responsibility |
|---|---|
| `threads` | intentions, facts, dependencies, queue state, outcomes |
| `agent-run-lifecycle` | run admission/hosting, graph driver operations, transport, live control, fallback/restoration evidence, and settlement |

The module instructions are in `guide.md`; each member's consumer entry
point is its `SKILL.md`. The switchboard definition lives in
`north:agent-catalog/north.json`, and the operator composes it as a separate
root beside the imported `agent-machinery` package. The member switches
remember their state, while an inactive coordination root
prevents any coordination instructions or skills from reaching a session.
Provider adapters that require static hook manifests consult the resolved North
generation at `~/.local/state/north/agents/current`, so the same containment
also suppresses
North lifecycle hooks without re-resolving activity.

Agent Machinery supplies acknowledged work-ownership transitions and the exact
eight-field portable run design. North then resolves provider, account, model,
and runtime, operates the concrete run, and settles it. Graph `driver` facts
remain North operational state; they do not establish actor ownership.
