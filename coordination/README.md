# Coordination module

Coordination is the shared-ledger half of North's optional orchestration
surface. It is a nested module with three members:

| Member | Responsibility |
|---|---|
| `messages` | durable requests, inboxes, urgent live delivery |
| `threads` | intentions, facts, dependencies, queue state, outcomes |
| `assignments` | the actor-to-thread ownership binding |

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

Coordination does not resolve roles or models. Orchestration supplies the
routing contract, staffing resolves a role profile, and assignments record
which actor owns the resulting concrete work.
