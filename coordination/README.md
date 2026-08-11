# Coordination modules

Coordination is the shared-ledger half of North's optional orchestration
surface. It is a nested set with three members:

| Member | Responsibility |
|---|---|
| `messages` | durable requests, inboxes, urgent live delivery |
| `threads` | intentions, facts, dependencies, queue state, outcomes |
| `assignments` | the actor-to-thread ownership binding |

The set instructions are in `guide.md`; each member's consumer entry
point is its `SKILL.md`. The switchboard definition lives in
`nixos-config:dotfiles/agents/modules.d/coordination.json`, and the outer
`orchestration` set contains this set. That containment is deliberate: the
member switches remember their state, while an inactive outer set prevents any
coordination instructions or skills from reaching a session. Provider adapters
that require static hook manifests consult the switchboard's derived
`~/.config/agents/activity.conf`, so the same containment also suppresses North
lifecycle hooks in Codex and Hermes.

Coordination does not choose roles or models. Orchestration makes the routing
decision, staffing instantiates a role profile, and assignments record who owns
the resulting concrete work.
