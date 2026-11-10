# Operator views: one read model, three layers

Tom said on 2026-11-10 that north's operator views overlap and confuse him: the same facts are computed and printed
by separate code, nothing discloses detail progressively, and nothing is layered by concern (north#1063). This note
lists every current view, the facts each prints and where they disagree, names the three layers, maps each view onto
them, and records the first fact moved to a single producer: an api organization's credit left. Use counts are the
30-day counts of [command usage](command-usage-2026-11.md).

## Names

Tom left the layer names to the coordinator (2026-11-10). They are plain words for what each layer is:

- **Glance** (layer 0): one line, `north glance`, and the same line as the fleet screen's top segment. It answers
  three questions: is anything broken, is money all right, does anything wait on Tom.
- **Board** (layer 1): the fleet screen (`north`) and `north dash`, one panel per concern, each opening on its
  headline numbers. The panels are **Work**, **Accounts**, **Infra** and **Health**.
- **Drill-down** (layer 2): one verb per panel, `north work`, `north accounts`, `north infra` and `north health`, each
  with `--json` carrying the same snapshot the board and the glance read.

The issue's working name for the second panel, "Capacity & money", is replaced by **Accounts**: in north capacity
means cores, memory and leases ([account usage](account-capacity.md) reserves "usage" for an account's allowance),
and every row of that panel is an account: a subscription's windows, an api organization's credit, and what was
spent on each. `north work`, `north infra` and `north health` already exist; the bare verb becomes the drill-down
and their subverbs stay.

## Every current view and what it prints

### Work

- `north` (the fleet screen): each session with its status, transcript and timers; the digest pane (Ctrl+T) with
  the open gates, each project's `status_command` line, each session's task and last report, and today's landings.
  Reads the fleet store, the books and each session's transcript.
- `north fleet list [--json]`, `north fleet tree`, `north fleet show NAME`: session rows, parent lineage, one session.
  Read the fleet store.
- `north fleet digest [--since 24h]` (24 uses): landed in the window, sessions in flight on every host, open gates
  with the recommended option, stalls, held messages. Reads the books, the cached session rows of every host and the
  gates elsewhere.
- `north fleet report` (4 uses): calibration and prior batches, the stall recovery line, spawn-gate denials and each
  project's goal score.
- `north ledger flow` (176 uses): throughput, stranded lanes, refused land branches, the lander and its alarms, and
  the usage yield (usage spent per landed first try, credit stranded or held back).
- `north dash` (265 uses): readiness bars, first tries, dispatch cells, escalations and Rust habits over a window.
- `north work list|show|...`: the work items and their holders.

### Accounts

- `north fleet usage` (194 uses): per subscription account every window (five-hour, week, scoped weekly including
  Fable), billing, resets, burn, plan and cost; then the api table (grant, expires, spent, left, later grants) and
  which hosts' spend it was charged with.
- `north fleet accounts`: the account labels (read by placement over ssh, so its format is a protocol);
  `--plan`: every plan account and api grant in placement order with its verdict line; `--api`: pool entries, their
  keys, exhausted marks and the entry launches take; `--reading ACCOUNT USD` records a console balance.
- `north fleet pace sample` (3 uses): samples token-only accounts through one request's rate-limit headers.
- `north dash`: per subscription the week left, use per hour, outlook and reset; the window's spend by model and
  effort as a share of each week; api runs and dollars in the window.
- `north fleet spend [NAME | --self]`: one session's API-equivalent spend.
- `north dash --weekly`, `north ledger runs` (107 uses): runs and their cost.
- `north ledger flow`: credit expired unspent, held back, and windows reset unspent while work waited.
- The fleet screen: nothing about accounts before this change, only the labels in the launch form.

### Infra

- `north infra [--json]`: each paid resource, its monthly price and one total; per machine its 24-hour load per
  core and the sessions and builds it served (north#1062).
- `north dash`: the billing section's machines, read from the same infrastructure manifest (`dash` imports
  `cli::infra::manifest`).
- `north capacity`: leases, modes and the watchdog per machine.

### Health

- `north health` (338 uses): harness invariants. `north runtime health` (410 uses): the installed runtimes.
- `north dash --bars`: readiness bars against their goals; `north check goals`: every goal's oracle.
- `north ledger flow --stall-alarm` and the lander alarms.

## Where they disagree

- **Api credit left** had four producers until this change, and no two agreed:
  - `credit::left`, read by placement through `src/cli/fleet/limit.rs`: the reading, else the current grant's
    estimate plus the later grants; charged with the spend of every pool name sharing the organization; closed when
    an exhausted mark file exists, whatever its date.
  - `credit::sources`, read by `north fleet accounts --plan`, dispatch's survey and `north ledger flow`: the
    reading, else the current grant only, later grants left out; charged with the first pool name's spend only.
  - `usage::credit_table`, read by `north fleet usage`: the reading, else the current grant only; first name only;
    printed "exhausted" once the list-price estimate reached zero, while placement kept launching on the
    organization because only the provider's refusal closes it.
  - `north dash` and the fleet screen printed no credit left at all.
  The split the issue records, one figure in `fleet accounts` and a far higher one in the console, was north's list-price estimate against the console;
  north#1043 added console readings, but the three producers above still differed whenever an organization had a
  later grant or two pool names. Now `north_pace::credit::standing` is the only producer: what is left on every
  unexpired grant, the reading where one is recorded, charged with every name's spend, closed only by a held mark.
  `north fleet usage`, `north fleet accounts --plan`, placement, dispatch, `north ledger flow`, `north dash` and the
  fleet screen's digest all read it, and `Standing::figure` words the figure once
  (`$X left (console reading TIME, estimate $Y)`).
- **Subscription windows** are read by two queries for the latest reading of each window: `usage/store.rs`
  (`north fleet usage`, `north dash`) and `latest_windows` in `src/cli/fleet/limit.rs` (placement and waits), which
  also derives limit events and the Fable fallback. They read the same rows, so the figures agree today; the second
  query is folded into the first when the Accounts panel is built.
- **Spend** is read from the same `runs` table by `fleet spend`, `dash`, `dash --weekly` and `ledger runs` over
  different windows at API-equivalent price; no disagreement is recorded.
- **Work in flight** is drawn three ways: the fleet screen follows the session rows of the fleet store, the digest
  reads every host's cached rows, and `ledger flow` reads runs. The Work panel reads one of them, the fleet store.

## Mapping onto the layers

- Glance: a projection of the snapshot. Health is the worst readiness bar and any red goal; money is the soonest
  account to run out before its reset and the api credit left; Tom is the count of open gates without a default.
- Board, Work: from `fleet digest` and the fleet screen's digest pane (sessions, gates, landed, stalls), with the
  first-try bar from `dash`.
- Board, Accounts: from `fleet usage`, `dash`'s billing section and `ledger flow`'s usage yield, every figure from
  the one account producer.
- Board, Infra: `north infra`'s total and idle machines.
- Board, Health: `dash --bars` and `north health`.
- Drill-down: `north work` (digest, list, tree, show, report and flow's lane sections), `north accounts` (usage,
  accounts, spend, runs and the usage yield), `north infra` (unchanged), `north health` (bars, invariants, goals).
- `fleet usage`, `fleet accounts --plan`, `fleet digest`, `fleet report` and `ledger flow` then print which view
  replaced them and retire by north#970's mechanism; `fleet accounts` without flags stays as the placement
  protocol, and `fleet spend --self` stays as the worker's budget read.

## The read model

One snapshot with four parts, each fact computed once from its source of truth, and every layer a projection:

- Work: the fleet store's session rows and the books (gates, landings, held messages).
- Accounts: the usage table's latest reading per window, and `credit::standing` per api organization.
- Infra: the infrastructure manifest and provider APIs as `north infra` reads them.
- Health: the readiness bars, the goals and the harness invariants.
