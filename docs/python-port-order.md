# Python port order

The order in which the Python in `harness/lib` moves to Rust, derived from the imports on main.
Run `rg -o '^\s*(from|import) (flow|usage|routing|goal_eta|project_score|project_plan|plan_usage|calibration)\b' -g '!*.test.py' harness/lib`
to regenerate the edges the cycle is built from; every edge it prints is in the list below.
The issues that own the ports are [#78](https://github.com/tompassarelli/north/issues/78) (usage family),
[#80](https://github.com/tompassarelli/north/issues/80) (placement, routing, calibration) and
[#81](https://github.com/tompassarelli/north/issues/81) (flow, report, routines, ledger context);
[#82](https://github.com/tompassarelli/north/issues/82) follows them and ports the `bin/agents` dispatcher.

## Imports between `harness/lib` modules

Each line is `importer -> imported`, one per module pair, counting imports inside functions.
A test file's imports travel with its module.

```
calibration -> defaults
claude_profiles -> usage
flow -> calibration
flow -> defaults
flow -> laptop_line
flow -> project_score
goal_eta -> calibration
goal_eta -> flow
goal_eta -> ledger_context
ledger_context -> defaults
ledger_context -> usage
plan_usage -> calibration
plan_usage -> defaults
plan_usage -> run_outcome
presence -> defaults
project_plan -> calibration
project_plan -> goal_eta
project_plan -> ledger_context
project_score -> calibration
project_score -> goal_eta
project_score -> plan_usage
project_score -> project_plan
project_score -> routing
project_score -> usage
report -> calibration
report -> policy_stats
report -> run_outcome
routing -> defaults
routing -> project_score
usage -> calibration
usage -> defaults
usage -> plan_usage
usage -> routing
usage -> ssh_transport
usage -> usage_claude
usage_claude -> claude_profiles
usage_claude -> usage
```

`policy_stats` is `harness/scripts/policy_stats.py`. Modules not named on the left import no other
`harness/lib` module: `billing_digitalocean`, `defaults`, `durable_events`, `laptop_line`,
`provider_capacity`, `routines`, `run_outcome`, `ssh_transport`,
`usage_agy`, `usage_codex`.

## The cycle

Ten modules reach each other: `claude_profiles`, `flow`, `goal_eta`, `ledger_context`, `plan_usage`,
`project_plan`, `project_score`, `routing`, `usage` and `usage_claude`. They belong to three issues
(#78: `claude_profiles`, `plan_usage`, `usage`, `usage_claude`; #80: `goal_eta`, `project_plan`,
`project_score`, `routing`; #81: `flow`, `ledger_context`), so no one issue can port the set alone, and
a port must not leave a Python module importing a Rust one.

Five edges close the cycle, and each exists to reach one small helper:

- `flow -> project_score` and `routing -> project_score` reach `missed_target`.
- `claude_profiles -> usage`, `usage_claude -> usage` and `ledger_context -> usage` reach
  `account_label`, `account_settings` and `local_account`.

Moving `missed_target` into a new leaf module beside `defaults` and the three account helpers into
another removes all five edges and leaves a graph with no cycle. In dependency order the layers are:

1. `billing_digitalocean`, `claude_profiles`, `defaults`, `durable_events`, `laptop_line`, `policy_stats`,
   `provider_capacity`, `routines`, `run_outcome`, `ssh_transport`,
   `usage_agy`, `usage_codex`
2. `calibration`, `ledger_context`, `presence`, `routing`, `usage_claude`
3. `flow`, `plan_usage`, `report`
4. `goal_eta`, `usage`
5. `project_plan`
6. `project_score`

## Python callers outside `harness/lib`

These import the cycle's modules directly and change in the same commit as the module they import:
`bin/agents`, `bin/threads`,
`harness/hooks/lib/codex_behavior.py`, `harness/hooks/lib/spawn_capacity.py`,
`harness/scripts/codex-route-propose.py`, `harness/scripts/harness-tuner.py`,
and `harness/scripts/policy-replay.py`.

## First port slice

Port the leaves #80 owns: `defaults`, `calibration`, `run_outcome`, `ssh_transport` and `laptop_line`.
None imports a cycle module (`calibration` imports only `defaults`), so they move first without the cycle
in the way. Callers become `north` subcommand calls in the same change, per the rules in #80.

The slice after it needs the cut: extract `missed_target` and the account helpers in Python, then port the
layers above in order, each layer with the issue that owns its modules (#78 for the usage family, #80 for
routing, planning and ETA, #81 for flow, report and ledger context).
