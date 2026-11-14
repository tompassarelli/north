# Terms

Four layers, one term each.

| Layer | What it is | North's instance | What swapping it changes |
| --- | --- | --- | --- |
| Model | The weights, text in and text out. | Whichever model a worker tier names. | The quality, speed and cost of each answer; not the tools or the roles. |
| Harness | The machinery that turns a model into an agent: loop, tools, context and guardrails. | The provider's agent runtime plus North's hooks, skills, routing and policy checks. | What an agent sees, what it can do and what stops it; the same model behaves differently. |
| Agent | One model running in a harness, in a loop, toward a goal. | A lead, a worker, the proxy. | One running instance and its goal; the layers below stay the same. |
| Fleet | Many agents with roles, a chain of command, staffing, budgets and policy. | North's sessions with their roles; South holds their private policy. | Who does what, with what authority and budget. |

## The swap test

Ask what the claim swaps out. If the weights, it's about the model. If the loop, tools or
guardrails, it's the harness. If one running instance with a goal, it's an agent. If who does what,
with what authority and budget, it's the fleet.

## Confusing cases

- "Sonnet does agent things" means a Sonnet worker: model, harness and loop together.
- An all-in-one agent product is a packaged harness with defaults and a persona.
- A **software factory** is the fleet plus the harness plus the landing path and the records:
  issues go in, verified landed changes come out, and what each run cost and where it went wrong
  is kept, so a loss that recurs becomes a rule. It is the four layers working together, not a
  fifth layer. North with the fleet it runs is one.

## NorthThread

A **NorthThread** is North's work-tracking primitive: an item with its claims, queue,
notes and ledger, managed through the `north threads` command and stored in `threads.db`.
In prose, use **NorthThread** or **NorthThreads**, never bare "thread(s)" or "North Threads"
for this primitive. Command and file names, environment variables, code identifiers,
and quoted commands keep their existing spelling. Provider conversation threads,
CPU threads and GitHub or mail discussion threads retain their ordinary names.

Before:
```text
Record it with `threads note REF TEXT`; native threads items never reach GitHub.
```
After:
```text
Record it with `threads note REF TEXT`; native NorthThread items never reach GitHub.
```

## Moving up the stack

The industry keeps moving its attention up the stack: first the model, then the agent, then the
harness, and now teams and orgs. Map any new term to one of these four layers instead of adopting it.
