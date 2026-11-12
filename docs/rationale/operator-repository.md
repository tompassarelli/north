# Operator repository: north carries the template, the private one passes the same check

Plan of 2026-11-12 for north#1281: north reads its operator's settings from a private repository nobody else can see, so a newcomer cannot tell what theirs must hold, and this note inventories every path north reads there, classes each, and decides how a public template and a private repository relate. Nothing is built here; the build boxes at the end are written onto the issue.

The private repository is described below only as kinds of file and counts. Every path in the inventory is one north's own code already names.

## Verdict

The issue's proposal holds, with four corrections from the inventory.

- Confirmed: no cleaned copy is ever published. North carries the template (the shape of every file it reads and a command that writes a working repository with synthetic values), and the private repository passes the same check a newcomer's does.
- Corrected: moving generic method out of the private repository is not a step of this plan and the privacy gate does not authorise it. The gate (deny-list, private-kinds judge, secret scan) is the second layer behind a decision that is the owner's. The template is written new in north and depends on no move.
- Corrected: the one piece that is provably generic needs no move. Of the catalog's 44 registrations, 38 name units north itself owns, so `north operator init` generates them from north's own structure registry.
- Added: a template is not enough for a newcomer while north finds the operator repository by a literal name and place, needs a third repository that is not public either, and needs an origin remote. Three build boxes close those.
- Added: nothing today refuses a file copied from the private repository into north unless it holds a listed value. Two build boxes add that refusal.

## How north finds the operator repository

There is no single locator. The default is the checkout `~/code/south/main`, with its operator directory `agents/` inside.

- The default is a literal in 41 non-test source files (60 lines).
- 15 environment variables and 3 flags (`--operator`, `--south-root`, `--south`) each move one reader or one group of readers: `AGENTS_OPERATOR_DIR`, `AGENTS_ORCHESTRATION`, `AGENTS_FROZEN_TOPICS`, `AGENT_POLICY_BOOTSTRAP`, `SAFE_PUSH_SOUTH`, `SAFE_PUSH_DENYLIST`, `NORTH_PRIVATE_KINDS`, `NORTH_PUBLIC_DENYLIST`, `SOUTH_SECRETS`, `NORTH_SOUTH_ROOT`, `NORTH_GROUNDING_SOUTH`, `NORTH_REPO_ROOTS`, `INCIDENT_RETRO_SOUTH`, `WATCH_TOML` and `WATCH_SOUTH`.
- Four readers have no override at all: the infra manifest (`src/cli/infra/manifest.rs:72`), host facts (`crates/north-runtime/src/runtime/host_facts.rs:112`), the shared-configuration snapshot's source (`crates/north-shared-config/src/lib.rs:48`) and the revision in the verdict key (`crates/north-landing-gates/src/landing/base_relative.rs:42`).
- The repository's name is also its identity. It is matched as a string to route its landing (`crates/north-safe-push/src/landing/south_ledger.rs:41`), to resolve catalog owners (`crates/north-core/src/agent_config_check/gate.rs:105`, `crates/north-core/src/catalog/paths.rs:26`), to keep it from the free model route (`harness/opencode.toml:6`) and to accept the remote that host facts are pushed to (`crates/north-core/src/catalog/live_refs_transport.rs:47`).

The name stays: "south" is the role's name in north's vocabulary ([docs/harness.md](../harness.md)), not a fact about the owner, and a newcomer's operator repository takes the same name and place by default.

## Inventory

Classes:

- **schema**: north parses the file. A newcomer needs its shape and a synthetic instance. Every value in the owner's copy stays private.
- **generic method**: content that reads the same for any operator and holds nothing private.
- **private value**: free-form facts about the owner. North reads it as text, searches it, or only names it. Nothing of it ships.
- **secret**: encrypted credentials. North reads them through sops or the broker. Nothing of it ships.

A file that holds two classes has two rows. Line numbers are at commit `c013d6124c`.

| # | Path under the operator repository | Read at | Class |
|---|---|---|---|
| 1 | `agents/orchestration.toml` | resolved in `crates/north-shared-config/src/lib.rs:54` and `crates/north-base/src/platform/orchestration.rs:9`; 59 non-test source files reach it | schema |
| 2 | `agents/costs.toml` | `crates/north-pace/src/router/choose.rs:28`, `crates/north-pace/src/usage/mod.rs:59`, `crates/north-pacing/src/pacing/placement/credit.rs:45`, `crates/north-flow/src/agents/status/mod.rs:136`, `crates/north-core/src/agent_config_check/accounts.rs:55`, `crates/north-threads/src/threads/outside.rs:54` | schema |
| 3 | `agents/claude-accounts.toml` | `crates/north-pace/src/credit.rs:49`, `:233` | schema |
| 4 | `agents/credentials.toml` | `crates/north-ops/src/ops/credentials/declared.rs:8`, `crates/north-broker/src/credentials.rs:51` | schema |
| 5 | `agents/frozen-topics.toml` | `crates/north-base/src/platform/frozen_topics.rs:35` | schema |
| 6 | `agents/operating-point.toml` | `crates/north-core/src/agent_config_check/gate.rs:266`, `crates/north-convo/src/policy_replay/point.rs:184`, `crates/north-runtime/src/timers/skill_prune.rs:131` | schema |
| 7 | `agents/watch.toml` | `crates/north-flow/src/detectors/gather/sources.rs:16`, `crates/north-flow/src/detectors/gather/skills.rs:16`, `crates/north-runtime/src/timers/watch_review/system.rs:15` | schema |
| 8 | `agents/rules.toml` | `crates/north-core/src/agent_config_check/rules.rs:178`, `crates/north-core/src/grounding/sources/policy.rs:419` | schema |
| 9 | `agents/incidents.jsonl` | `crates/north-core/src/agent_config_check/rules.rs:184` | schema |
| 10 | `agents/catalog-config.json`: activation, baselines, provider support, registrations of units the operator owns | `crates/north-core/src/catalog/paths.rs:16`, `crates/north-core/src/agent_policy_contract.rs:109`, `crates/north-core/src/agent_config_check/gate.rs:285`, `crates/north-core/src/catalog/registration_checks.rs:206`, `crates/north-runtime/src/timers/skill_prune.rs:116` | schema |
| 11 | `agents/catalog-config.json`: registrations of units north owns | the readers of row 10, resolved against north's structure registry in `crates/north-core/src/catalog/owner_modules.rs:17` | generic method |
| 12 | `agents/routines/*.md` (frontmatter) | `crates/north-flow/src/agents/routines/mod.rs:42`, `crates/north-runtime/src/job_watch/mod.rs:28`, `crates/north-core/src/agent_config_check/mod.rs:305` | schema |
| 13 | `agents/replay/` (cases and sweeps) | `crates/north-core/src/agent_config_check/gate.rs:164`, `:193` | schema |
| 14 | `privacy/public-denylist.toml` | `crates/north-landing-gates/src/landing/denylist_mirror.rs:12`, `crates/north-guard/src/guard/public_text.rs:10`, `crates/north-land-scans/src/outgoing/denylist.rs:113` | schema |
| 15 | `privacy/private-kinds.toml` | `crates/north-runtime/src/privacy/kinds.rs:8` | schema |
| 16 | `infra/manifest.toml` | `src/cli/infra/manifest.rs:11` | schema |
| 17 | `agents/AGENTS.md` (the operator's global rules) | `crates/north-core/src/agent_config_check/gate.rs:265`, `crates/north-core/src/agent_config_check/rules.rs:177`, `crates/north-core/src/agent_policy_contract.rs:113`, `crates/north-convo/src/policy_replay/sweep.rs:141`, `harness/mayday/tier0.list:5` | private value |
| 18 | `agents/skills/` (skills the operator owns) | each registration's owner path, `crates/north-core/src/catalog/paths.rs:58` | private value |
| 19 | `agents/decisions/` | named as the place a durable record goes, `crates/north-guard/src/guard/handoff_record.rs:7`; read only through row 20 | private value |
| 20 | every tracked file under `agents/`, as text | the prompt-grounding corpus, `crates/north-core/src/grounding/sources/policy.rs:472` | private value |
| 21 | the whole checkout, as text | the vocabulary check, `crates/north-core/src/agent_config_check/mod.rs:258` | private value |
| 22 | `hosts/` | searched for uses of north's commands, `crates/north-landing-gates/src/landing/bin_references.rs:154` | private value |
| 23 | `modules/` | the same search | private value |
| 24 | `bin/` | the same search | private value |
| 25 | the checkout's Git facts: origin URL, main's revision, a fetch of origin's main | `crates/north-land-scans/src/outgoing/denylist.rs:172`, `crates/north-landing-gates/src/landing/denylist_mirror.rs:63`, `crates/north-landing-gates/src/landing/base_relative.rs:42`, `crates/north-shared-config/src/lib.rs:92`, `crates/north-runtime/src/runtime/checkouts.rs:26` | private value |
| 26 to 31 | six encrypted files, one per service the broker calls, each named by a constant in north | `crates/north-broker/src/credentials.rs:11` to `:16` | secret |
| 32 | every encrypted file under the secrets directory, listed and fingerprinted, never decrypted | `crates/north-ops/src/ops/credentials/declared.rs:102` | secret |

Counts per class: **schema 15, generic method 1, private value 9, secret 7; 32 rows.**

North also writes into the operator repository on five paths, each through a session or a verb of its own: incident rows and rule candidates (`crates/north-runtime/src/timers/incident_retro/mod.rs:58`), the watch list (`crates/north-runtime/src/timers/watch_review/mod.rs:67`), replay sweeps (`crates/north-convo/src/policy_replay/sweep.rs:157`), host facts (`crates/north-runtime/src/runtime/host_facts.rs:13`) and the landing verb for a host whose ledger lives elsewhere (`crates/north-ops/src/ops/remote_serve/south_land.rs`).

### The private tree, by kind

941 tracked files. By the kind of thing each is:

- decision records: 592
- routines: 90
- replay cases and sweeps: 87
- the secrets tree: 57, of which 49 are encrypted files
- machine modules: 20, and host configurations: 16
- operator-owned skills: 20 files in 7 skills
- privacy lists, their fixtures and their tests: 13
- top-level policy files in the operator directory: 12, of which north reads 11
- scripts: 5, and the infra manifest with its note: 2
- files of kinds north has no reader for (repository plumbing, tests, personal configuration and files kept for other projects): 27

Three kinds hold method a newcomer would value and cannot be classed from paths alone: the rule register (94 rules and 25 candidates), the routines (90) and the replay cases (87). Classing them means reading and describing them, which this plan does not do. They stay private until the owner rules on each kind.

### What the inventory shows beyond the table

- A newcomer with no operator checkout lands unscanned: the deny-list and the judge warn and pass (`crates/north-land-scans/src/outgoing/denylist.rs:185`, `:205`). A template that writes starter lists protects them from the first landing.
- `agents sync` refreshes three checkouts in order, the operator's, north's and the machine-configuration repository's, and publishes nothing when one cannot be fetched (`crates/north-core/src/catalog/sync_gate.rs:44` to `:47`, `:160`). A newcomer does not have the third, and a repository made a minute ago has no origin to fetch.
- The catalog's `$schema` names a file north does not hold.
- North already holds synthetic copies of two of the 15 schema files as test fixtures (`harness/lib/fixtures/orchestration.toml`, `harness/lib/fixtures/costs.toml`), so init starts from shapes the tests already exercise.
- North's own skills and docs point into the operator repository 31 times at 14 distinct places, and [docs/harness.md](../harness.md) says the full harness is not a public install. Those belong to the docs boxes and the privacy sweep of north#1281.

## Decision

North carries the template, with an init and a check command (alternative a). A generated example tree is committed inside north and checked equal to init's output, which takes the one benefit of a published example without a second repository.

Costs are counted in build boxes of at most 40 minutes each; the box lists are estimates from the reader sites counted above, not measurements.

### a. The template inside north, with `north operator init` and `north operator check` (chosen)

- What changes: a registry of operator paths in north (the inventory above as code), a typed schema per schema-class file, an init that writes synthetic values, a check that composes what already exists (`north check harness policy-gate --operator`, `north config agents check-owners`, `north ops credentials check`). No reader changes for a newcomer who takes the default place; the locator boxes then put every reader behind one resolver.
- Every future edit: a reader that adds or drops a key edits the schema in the same change, and north's landing runs the check against the owner's checkout so a change that would break it is refused.
- Cost: 17 boxes, at most 11 worker-hours; 14 of them, at most 9 worker-hours, are the path to a working first hour.
- Privacy: no byte is copied from the private repository. The remaining risk is an author modelling a synthetic value on a real one, which the deny-list catches for listed values and the new same-value check catches for every other.
- First hour: one clone and one command, and the template always matches the binary that wrote it.

### b. A separate public base repository that a private one layers over (rejected)

- The 15 schema files are almost wholly values: accounts, hosts, prices, lists. Layered file by file, the private file replaces the base file whole and the base gives a newcomer nothing an example would not. Layered key by key, a public repository feeds keys into the files trust decisions read (`orchestration::snapshot`), so a host or an account the private file forgot to override would come from a public source.
- As a flake input it does not reach the readers: north reads checkout paths, a fetched ref and `git show`, never a Nix output. About 20 reader sites outside the two orchestration resolvers would need a layered lookup, and the shared-configuration snapshot would need to fetch and merge two sources at one revision each.
- As a subtree, the private repository gains a remote that points at a public repository. One push of the wrong ref publishes private history, the worst failure any alternative has, and it is one command away for every later worker.
- Every future edit decides a layer and a merge rule, and three things must agree on a version: the binary, the base and the private repository.
- Cost: the 17 boxes of (a), since the schema and the check are still needed, plus about 22 for layered lookup, the two-source snapshot, merge rules for 15 files, the base repository and its pin: about 39 boxes, 26 worker-hours.
- First hour: three repositories and the idea of layering before the first command.

### c. A generated public example repository published from the schema (rejected)

- It is (a) plus a publisher: a new public repository, a job that pushes to it, a credential for that job in the registry, and an alarm when it goes stale.
- What it adds for a newcomer is an example to browse before building. The committed example tree gives that inside north, at the same commit as the binary.
- What it adds in risk is a job that pushes a directory to a public repository from a host that also holds the private one, and a second place whose version can differ from the north a newcomer cloned.
- Cost: (a) plus about 4 boxes, 21 boxes and 14 worker-hours, a recurring job to own, and a new public repository, which is the owner's decision.

### Which serves what

- A newcomer's first hour: (a). One repository, one command, no version to match.
- The private data: (a). It has no copy step and no push path. (c) is equal in what is copied and worse by one push path; (b) is worst.

## The private side

What the private repository changes to consume the template:

1. A schema marker: one file at its root holding the template's version, which `north operator init` writes for a newcomer.
2. One line in its own landing configuration that runs `north operator check .`, so a landing that breaks the shape is refused there.

The order that never leaves it broken:

1. North lands the registry and the schemas. Version 1 is descriptive: before each schema box lands, its check runs against the owner's checkout on a host that holds it, and each finding is either a schema error, fixed in the box, or a real fault, fixed first in the private repository by a landing of its own.
2. The private repository adds the marker and the check line. They pass because of step 1.
3. North's landing gains a machine-bound check that runs `north operator check` on the owner's checkout when the registry or a schema changes, the way `check-owners` already reads that checkout ([.safe-push](../../.safe-push)).
4. After that, a change of shape takes three landings, the rule [harness-skill-roots.md](harness-skill-roots.md) reached for cross-repository moves. North lands version N+1 while still accepting N. The private repository adds or drops the key and raises its marker. North then drops N, in a landing whose machine-bound check confirms the marker. North accepts two versions only between the first and the third landing of one change.

A new optional key needs none of this: north lands it and the private repository takes it when it wants.

## What never moves

These kinds stay private by rule. A later worker does not move one; a proposal to move one is a question for the owner, asked once per kind with the count and what the template lacks without it.

- the secrets tree: encrypted files, the configuration that names who can decrypt them, and keys
- the deny-list, which is an index of the private values themselves
- decision records, postmortems and the incident ledger
- account, cost, credential-declaration and infra files
- host configurations and machine modules
- skills the operator owns
- the operator's global rules, the rule register, routines and replay cases, until the owner rules on a kind
- replay sweeps and the operating point, which are measured on the owner's own policy

How the landing enforces it, in layers. The first three exist today for every public destination:

1. The deny-list scan refuses a listed value ([private-text-judge.md](private-text-judge.md)).
2. The private-kinds judge refuses a paragraph that names no listed value but is of a private kind.
3. The secret scan refuses key material ([betterleaks.md](betterleaks.md)).
4. New, the private-file refusal: for a public destination, safe-push refuses an added file whose content is byte for byte a file anywhere in the operator repository's history, unless the destination already held it or an approved-moves list in the operator repository names it. It is exact, asks no model and reaches old commits. The approved-moves list is how an owner's decision becomes mechanical: the decision is a landing in the private repository, and nothing else unlocks the move.
5. New, the same-value check: on a host that holds the owner's checkout, a change to the template is refused when a value it writes at a private-class key equals the owner's value at that key. The refusal names the key and never the value.

The registry carries each path's class, so layers 4 and 5 read which paths are private from one place.

## Build boxes

Written onto north#1281 under its "Operator repository, built" box. Each is at most 40 minutes. `D` is a new scratch directory.

1. Registry: `north operator paths` prints one line per inventory row with its class. Done when: `north operator paths | wc -l` prints 32 and `north operator paths --class secret | wc -l` prints 7.
2. Schemas for the account and cost files (rows 2 to 4 and 16). Done when: `north operator check --only costs,claude-accounts,credentials,infra` exits 0 on the owner's checkout and exits 1 naming file and key on a scratch copy with one key misspelled.
3. Schemas for the policy files (rows 5 to 9, 12, 13 and 15). Done when: the same two runs for `--only policy`.
4. Schema for orchestration, the account side (the account and tier tables). Done when: the same two runs for `--only orchestration-accounts`.
5. Schema for orchestration, the host side (hosts, projects, placement, dispatch and every remaining key). Done when: `north operator check --only orchestration` exits 0 on the owner's checkout and exits 1 on a scratch copy with an unknown table.
6. Catalog: the schema file at the place the catalog's `$schema` names, and registrations generated from north's structure registry. Done when: `north operator init D && north config agents check-owners --north-root . --south-root D` exits 0 with no machine-configuration root named.
7. Init, configuration: every schema-class file with synthetic values, starter privacy lists, a secrets directory holding only a note, the marker and a landing configuration. Done when: `north operator init D && north operator check D` exits 0 and a second init into another directory gives the same tree by `diff -r`.
8. Init, policy: starter global rules written new in north, with their register, so the policy gate passes with no replay case. Done when: `north check harness policy-gate session --operator D/agents` exits 0.
9. Sync with an operator repository that has no origin and with no machine-configuration checkout. Done when: in a scratch home holding north's runtime and init's output, `agents sync` exits 0 and the published generation exists.
10. Example tree: `examples/operator/` equals init's output and the landing checks it. Done when: `north operator init --check examples/operator` exits 0 on main and exits 1 after one byte of the example changes.
11. Private side, in the private repository: the marker and the check line. Done when: `north operator check .` exits 0 at its main, with the run recorded there.
12. North's landing protects the owner's checkout. Done when: a drill lane that makes one optional key required is refused at landing with the key named.
13. Private-file refusal. Done when: a drill lane that copies one file from a scratch operator repository into a scratch public one is refused with the path named, lands once the approved-moves list names it, and a run over main's last 200 commits prints each hit for the privacy sweep.
14. Same-value check. Done when: a drill template holding one value copied from a scratch operator repository is refused with the key named and the value absent from the output.
15. One locator, policy and catalog readers: every reader takes the operator root from one resolver and one variable. Done when: `git grep -l "code/south" -- crates/north-core crates/north-base crates/north-shared-config` lists only the resolver.
16. One locator, landing, scan and guard readers. Done when: the same search over `crates/north-landing-gates crates/north-land-scans crates/north-safe-push crates/north-guard crates/north-broker` lists nothing.
17. One locator, the rest, and a lint that refuses the literal outside the resolver. Done when: `git grep -l "code/south" -- crates src ':!*test*'` lists only the resolver, and a drill lane that adds the literal elsewhere is refused.

Boxes 1 to 9 give `north operator init` and a working first hour; 10 to 14 hold it and the private data in place; 15 to 17 can follow the weekend.

## Not required

- Moving any file out of the private repository. The template needs none.
- A migration or upgrade command for an operator repository made by an older north. The check names what is missing.
- Machine configuration for a newcomer's hosts. That belongs to the machine-configuration repository, which this plan only makes optional for `agents sync`.
- Renaming the operator repository's role.
