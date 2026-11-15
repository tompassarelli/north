# Verb inventory by use, 2026-11-15

Measured for north#1487, extending [command usage](command-usage-2026-11.md), whose rule stands: zero use alone never retires a verb. Its unused list is below.

Unused verbs: agent-contain sweep, autoland-status, capacity game-reserve, capacity gaming, capacity seat-idle-hint, denylist scan, fleet orders, graph retract, graph validate, hardcoded-repo-path-check, land fails-first-runs, land host-gate, land mutant-runs, land static-gates, lane audit, outside-watch status, placement-survey keep, provider-capacity events, provider-capacity status, spare-retire, ssh-argv, ssh-spare hold, work close, work need, work release, work unneed.


Method: 30-day invocations are the text north followed by the verb calls in Bash tool commands of every Claude transcript modified in 30 days under `~/.local/state/north/profiles/claude-{a,c,d,e}/projects` on nexus (792 files, 27,138 commands; Codex transcripts and other hosts' trees are not readable from a worker, and the convo index has not been built on this host). A one-word verb counts every subverb under it. Callers are the files' the text north followed by the verb mentions counted in nixos-config `modules` and `hosts` (units), north `harness/claude`, `harness/codex`, `harness/scripts`, `harness/lib` and `crates/north-guard/src/hook.rs` (hooks), `harness/skills` (skills), `bin` and `lib` (bin), and `docs`, `README.md`, `ARCHITECTURE.md`, `AGENTS.md` (docs). Verbs are the 300 leading verb words of `north --help-all`.

| verb | 30d | units | hooks | skills | bin | docs |
|---|---:|---:|---:|---:|---:|---:|
| hook | 1936 | 2 | 2 | 2 | 0 | 74 |
| fleet | 634 | 5 | 8 | 24 | 1 | 409 |
| capacity dev-shell | 287 | 1 | 2 | 0 | 0 | 9 |
| lane new | 216 | 0 | 0 | 1 | 0 | 4 |
| lane remove | 214 | 0 | 0 | 2 | 0 | 10 |
| capacity host | 210 | 3 | 0 | 11 | 0 | 19 |
| gate | 161 | 0 | 2 | 4 | 0 | 30 |
| capacity offload | 153 | 1 | 2 | 4 | 0 | 41 |
| task | 138 | 0 | 0 | 0 | 0 | 16 |
| fleet need | 97 | 0 | 5 | 1 | 0 | 18 |
| land status | 81 | 0 | 0 | 0 | 0 | 1 |
| land scan | 47 | 0 | 0 | 5 | 0 | 43 |
| threads | 35 | 0 | 0 | 6 | 0 | 59 |
| land profile | 29 | 0 | 0 | 0 | 0 | 25 |
| code-health crate-lines | 27 | 0 | 1 | 0 | 0 | 3 |
| job-run | 27 | 4 | 0 | 3 | 0 | 5 |
| code-health report | 26 | 0 | 0 | 0 | 0 | 16 |
| ledger opencode | 24 | 0 | 0 | 0 | 0 | 3 |
| check experiments | 23 | 0 | 0 | 1 | 0 | 12 |
| convo | 23 | 2 | 1 | 15 | 0 | 14 |
| check harness | 22 | 0 | 0 | 2 | 0 | 13 |
| dash | 22 | 0 | 0 | 0 | 0 | 70 |
| accounts | 21 | 0 | 0 | 0 | 0 | 18 |
| buck targets | 21 | 0 | 0 | 0 | 0 | 7 |
| config agents | 21 | 2 | 0 | 0 | 0 | 34 |
| test mutant | 21 | 1 | 0 | 1 | 1 | 5 |
| docs structure-index | 20 | 0 | 0 | 0 | 0 | 2 |
| fleet usage | 20 | 2 | 0 | 0 | 0 | 35 |
| lane gc | 20 | 1 | 0 | 5 | 0 | 7 |
| fleet dispatch | 19 | 0 | 0 | 4 | 0 | 54 |
| tune | 19 | 0 | 0 | 0 | 0 | 41 |
| ledger flow | 18 | 1 | 0 | 5 | 0 | 75 |
| runtime health | 18 | 0 | 0 | 0 | 0 | 3 |
| depbox | 17 | 0 | 0 | 1 | 1 | 18 |
| lane exec | 16 | 0 | 0 | 0 | 2 | 1 |
| health | 15 | 1 | 0 | 0 | 0 | 48 |
| code-health check | 14 | 0 | 0 | 0 | 0 | 0 |
| code-health habits | 13 | 0 | 0 | 1 | 0 | 5 |
| runtime status | 13 | 0 | 0 | 0 | 0 | 5 |
| check machines | 12 | 0 | 0 | 1 | 0 | 50 |
| code-health layers | 12 | 0 | 0 | 0 | 0 | 14 |
| graph ask | 12 | 0 | 0 | 2 | 0 | 25 |
| docs features-index | 11 | 0 | 0 | 0 | 0 | 10 |
| hook prompt-grounding | 10 | 0 | 0 | 0 | 0 | 3 |
| land missed | 10 | 0 | 0 | 0 | 0 | 13 |
| lane seed | 10 | 0 | 1 | 0 | 0 | 17 |
| runtime accept | 9 | 0 | 0 | 0 | 0 | 1 |
| work | 9 | 0 | 2 | 3 | 0 | 40 |
| docs rationale-index | 8 | 0 | 0 | 0 | 0 | 9 |
| graph ready | 8 | 0 | 0 | 0 | 0 | 10 |
| lane rescue | 8 | 0 | 0 | 0 | 0 | 2 |
| safe-push | 8 | 0 | 0 | 0 | 0 | 12 |
| capacity mode | 6 | 0 | 0 | 6 | 0 | 2 |
| code-health signal | 6 | 0 | 0 | 0 | 0 | 1 |
| fleet laptop | 6 | 1 | 0 | 1 | 0 | 2 |
| fleet reap | 6 | 0 | 0 | 0 | 0 | 4 |
| graph ingest | 6 | 0 | 0 | 0 | 0 | 14 |
| land promote | 6 | 1 | 0 | 0 | 0 | 18 |
| lane sync | 6 | 0 | 0 | 0 | 0 | 2 |
| docs reference-index | 5 | 0 | 0 | 0 | 0 | 1 |
| capacity model | 4 | 0 | 0 | 0 | 0 | 5 |
| check citations | 4 | 0 | 0 | 0 | 0 | 20 |
| check issue-boxes | 4 | 0 | 0 | 0 | 0 | 1 |
| lane stray | 4 | 0 | 0 | 0 | 0 | 2 |
| runtime lag | 4 | 0 | 0 | 0 | 0 | 12 |
| watch-review | 4 | 1 | 0 | 1 | 0 | 2 |
| desktop warcraft | 3 | 0 | 0 | 1 | 0 | 4 |
| fleet digest | 3 | 0 | 0 | 0 | 0 | 3 |
| land doctor | 3 | 1 | 0 | 0 | 0 | 16 |
| ledger worker | 3 | 1 | 0 | 4 | 0 | 44 |
| runtime build | 3 | 0 | 0 | 0 | 0 | 3 |
| work brief | 3 | 0 | 0 | 3 | 0 | 17 |
| work show | 3 | 0 | 0 | 0 | 0 | 1 |
| check disclosure | 2 | 0 | 0 | 0 | 0 | 1 |
| code-health gate | 2 | 0 | 0 | 0 | 0 | 1 |
| code-health review-done | 2 | 0 | 0 | 0 | 0 | 0 |
| finish | 2 | 0 | 0 | 2 | 0 | 13 |
| fleet spool | 2 | 1 | 0 | 0 | 0 | 4 |
| land abandon | 2 | 0 | 0 | 1 | 0 | 1 |
| land reuse | 2 | 0 | 0 | 0 | 0 | 12 |
| land serve | 2 | 2 | 0 | 0 | 0 | 22 |
| land urgent | 2 | 0 | 0 | 0 | 0 | 4 |
| lane reclaim | 2 | 3 | 0 | 0 | 0 | 10 |
| ledger runs | 2 | 0 | 0 | 0 | 0 | 3 |
| ops check | 2 | 7 | 0 | 0 | 0 | 7 |
| runtime checkouts | 2 | 0 | 0 | 0 | 0 | 4 |
| accounts reset | 0 | 0 | 0 | 0 | 0 | 3 |
| after-landing | 0 | 0 | 0 | 3 | 0 | 8 |
| agent-contain sweep | 0 | 0 | 0 | 0 | 0 | 0 |
| agent-policy-contract | 0 | 0 | 0 | 0 | 0 | 1 |
| alarm stats | 0 | 0 | 0 | 0 | 0 | 1 |
| autoland-status | 0 | 0 | 0 | 0 | 0 | 0 |
| bin-references | 0 | 0 | 0 | 0 | 0 | 1 |
| box path | 0 | 0 | 0 | 0 | 0 | 3 |
| broker github-token | 0 | 0 | 0 | 0 | 1 | 2 |
| broker inspect | 0 | 0 | 0 | 0 | 0 | 1 |
| broker serve | 0 | 1 | 0 | 0 | 0 | 2 |
| buck compare-cases | 0 | 0 | 0 | 0 | 0 | 5 |
| buck host-cases | 0 | 0 | 0 | 0 | 0 | 2 |
| build-cache-gate | 0 | 0 | 0 | 0 | 0 | 1 |
| capacity actions-pull | 0 | 0 | 0 | 0 | 0 | 5 |
| capacity admission | 0 | 0 | 0 | 1 | 0 | 0 |
| capacity bounded | 0 | 0 | 0 | 1 | 0 | 3 |
| capacity calibration | 0 | 0 | 0 | 0 | 0 | 8 |
| capacity disk | 0 | 0 | 0 | 0 | 0 | 7 |
| capacity disk-share | 0 | 0 | 0 | 0 | 0 | 2 |
| capacity game-reserve | 0 | 0 | 0 | 0 | 0 | 0 |
| capacity gaming | 0 | 0 | 0 | 0 | 0 | 0 |
| capacity laptop-auto | 0 | 1 | 0 | 0 | 0 | 1 |
| capacity nexus | 0 | 0 | 0 | 0 | 0 | 2 |
| capacity pull | 0 | 0 | 0 | 0 | 0 | 2 |
| capacity reap-orphans | 0 | 0 | 0 | 0 | 0 | 2 |
| capacity seat-idle-hint | 0 | 0 | 0 | 0 | 0 | 0 |
| capacity watchdog | 0 | 1 | 0 | 0 | 0 | 3 |
| check calls | 0 | 0 | 0 | 0 | 0 | 1 |
| check concerns | 0 | 0 | 0 | 0 | 0 | 9 |
| check nix | 0 | 0 | 0 | 0 | 0 | 1 |
| check oneshot | 0 | 0 | 0 | 0 | 0 | 1 |
| check provenance | 0 | 0 | 0 | 0 | 0 | 5 |
| ci-diagnose | 0 | 0 | 0 | 0 | 0 | 5 |
| cloud collect | 0 | 0 | 0 | 0 | 0 | 3 |
| cloud events | 0 | 0 | 0 | 1 | 0 | 0 |
| cloud land | 0 | 0 | 0 | 0 | 0 | 10 |
| cloud launch | 0 | 0 | 0 | 2 | 0 | 6 |
| cloud list | 0 | 0 | 0 | 1 | 0 | 1 |
| cloud reap | 0 | 0 | 0 | 1 | 0 | 1 |
| cloud send | 0 | 0 | 0 | 1 | 0 | 0 |
| cloud sweep | 0 | 0 | 0 | 1 | 0 | 4 |
| cloud wait | 0 | 0 | 0 | 1 | 0 | 4 |
| code-health build-time | 0 | 1 | 0 | 0 | 0 | 6 |
| code-health domains | 0 | 0 | 0 | 0 | 0 | 2 |
| code-health split | 0 | 0 | 0 | 0 | 0 | 6 |
| codex-runtime-gcroots | 0 | 1 | 0 | 0 | 0 | 0 |
| config check | 0 | 0 | 0 | 9 | 0 | 1 |
| config context | 0 | 0 | 0 | 0 | 0 | 8 |
| config coordinator | 0 | 0 | 1 | 2 | 0 | 3 |
| config operator | 0 | 0 | 0 | 0 | 0 | 20 |
| contract-probe | 0 | 0 | 0 | 0 | 0 | 0 |
| control-plane gate | 0 | 0 | 0 | 0 | 0 | 2 |
| control-plane move | 0 | 0 | 0 | 0 | 0 | 5 |
| control-plane where | 0 | 0 | 0 | 0 | 0 | 3 |
| coordinator | 0 | 0 | 0 | 2 | 0 | 39 |
| coordinator move | 0 | 0 | 0 | 0 | 0 | 3 |
| cross-project-scan | 0 | 1 | 0 | 0 | 0 | 1 |
| defaults-measure | 0 | 0 | 0 | 0 | 0 | 8 |
| denylist attest | 0 | 1 | 0 | 0 | 0 | 0 |
| denylist scan | 0 | 0 | 0 | 0 | 0 | 0 |
| dep-gate | 0 | 0 | 0 | 3 | 0 | 1 |
| desktop capture | 0 | 0 | 0 | 0 | 0 | 1 |
| desktop-idle-state | 0 | 1 | 0 | 0 | 0 | 0 |
| desktop private | 0 | 0 | 0 | 3 | 0 | 3 |
| docs help-index | 0 | 0 | 0 | 0 | 0 | 1 |
| docs readme | 0 | 0 | 0 | 0 | 0 | 8 |
| docs verbs-index | 0 | 0 | 0 | 0 | 0 | 3 |
| eta-calibration | 0 | 1 | 0 | 0 | 0 | 1 |
| evidence start | 0 | 0 | 0 | 0 | 0 | 2 |
| fleet break-glass | 0 | 0 | 1 | 1 | 0 | 5 |
| fleet drafts | 0 | 0 | 0 | 0 | 0 | 3 |
| fleet held | 0 | 0 | 0 | 0 | 0 | 5 |
| fleet orders | 0 | 0 | 0 | 0 | 0 | 0 |
| fleet pace | 0 | 0 | 0 | 0 | 0 | 13 |
| fleet report | 0 | 0 | 0 | 0 | 0 | 5 |
| fleet wait | 0 | 0 | 1 | 1 | 0 | 7 |
| fleet waits | 0 | 0 | 0 | 0 | 0 | 11 |
| follows-release | 0 | 0 | 0 | 0 | 0 | 4 |
| gh-broker serve | 0 | 1 | 0 | 0 | 0 | 1 |
| gh-session | 0 | 0 | 0 | 1 | 2 | 5 |
| git-credential erase | 0 | 0 | 0 | 0 | 0 | 0 |
| git-credential get | 0 | 0 | 0 | 0 | 0 | 0 |
| git-credential store | 0 | 0 | 0 | 0 | 0 | 0 |
| github-mirror serve | 0 | 0 | 0 | 0 | 0 | 1 |
| github-mirror sync | 0 | 0 | 0 | 0 | 0 | 1 |
| git-maintenance-nightly | 0 | 1 | 0 | 0 | 0 | 0 |
| glance | 0 | 0 | 0 | 0 | 0 | 2 |
| graph ground | 0 | 0 | 0 | 0 | 0 | 11 |
| graph retain | 0 | 2 | 0 | 0 | 0 | 7 |
| graph retract | 0 | 0 | 0 | 0 | 0 | 0 |
| graph show | 0 | 0 | 0 | 0 | 0 | 2 |
| graph split | 0 | 0 | 0 | 0 | 0 | 7 |
| graph tell | 0 | 0 | 0 | 0 | 0 | 2 |
| graph validate | 0 | 0 | 0 | 0 | 0 | 0 |
| guard language | 0 | 0 | 0 | 0 | 0 | 1 |
| hand-in | 0 | 0 | 0 | 1 | 0 | 5 |
| hardcoded-repo-path-check | 0 | 0 | 0 | 0 | 0 | 0 |
| help | 0 | 0 | 0 | 0 | 0 | 0 |
| holds list | 0 | 0 | 0 | 0 | 0 | 2 |
| home move | 0 | 0 | 0 | 0 | 0 | 3 |
| hook recommend-acts | 0 | 0 | 0 | 0 | 0 | 2 |
| hook stats | 0 | 0 | 0 | 0 | 0 | 13 |
| hook stop | 0 | 0 | 0 | 0 | 0 | 4 |
| incident-mine | 0 | 0 | 0 | 0 | 0 | 5 |
| incident-retro | 0 | 1 | 0 | 0 | 0 | 2 |
| infra | 0 | 0 | 0 | 0 | 0 | 10 |
| issue convert | 0 | 0 | 0 | 0 | 0 | 4 |
| job-watch run | 0 | 1 | 0 | 0 | 0 | 1 |
| land actions | 0 | 0 | 0 | 0 | 0 | 3 |
| land adopt | 0 | 0 | 0 | 0 | 0 | 2 |
| land break-glass | 0 | 0 | 0 | 0 | 0 | 5 |
| land clean | 0 | 0 | 0 | 0 | 0 | 1 |
| land clear-withdrawal | 0 | 0 | 0 | 0 | 0 | 1 |
| land dispatch | 0 | 0 | 0 | 0 | 0 | 2 |
| land drill | 0 | 2 | 0 | 0 | 0 | 8 |
| landed-tree | 0 | 0 | 0 | 0 | 0 | 0 |
| land fails-first | 0 | 0 | 0 | 0 | 0 | 3 |
| land fails-first-runs | 0 | 0 | 0 | 0 | 0 | 0 |
| land follow | 0 | 0 | 0 | 0 | 0 | 10 |
| land freeze | 0 | 0 | 0 | 0 | 0 | 4 |
| land gate-stats | 0 | 0 | 0 | 0 | 0 | 10 |
| land host-gate | 0 | 0 | 0 | 0 | 0 | 0 |
| land machinery-replay | 0 | 0 | 0 | 0 | 0 | 1 |
| land mutant-runs | 0 | 0 | 0 | 0 | 0 | 0 |
| land red-check | 0 | 0 | 0 | 0 | 0 | 2 |
| land release | 0 | 0 | 0 | 0 | 0 | 7 |
| land requeue | 0 | 0 | 0 | 0 | 0 | 2 |
| land static-gates | 0 | 0 | 0 | 0 | 0 | 0 |
| land tests | 0 | 0 | 0 | 0 | 0 | 8 |
| land train | 0 | 0 | 0 | 0 | 0 | 9 |
| land withdraw | 0 | 0 | 0 | 0 | 0 | 2 |
| lane audit | 0 | 0 | 0 | 0 | 0 | 0 |
| lane land-draft | 0 | 0 | 0 | 0 | 0 | 2 |
| lane layout | 0 | 0 | 0 | 0 | 0 | 1 |
| lane reap | 0 | 0 | 0 | 0 | 0 | 1 |
| lane restore-packed-refs | 0 | 0 | 0 | 0 | 0 | 1 |
| lane squash | 0 | 0 | 0 | 0 | 0 | 2 |
| lane unlock | 0 | 0 | 0 | 0 | 0 | 3 |
| language-gate | 0 | 0 | 0 | 2 | 0 | 0 |
| laptop-runner | 0 | 1 | 0 | 0 | 0 | 1 |
| ledger duplicate | 0 | 0 | 0 | 0 | 0 | 1 |
| ledger grounding | 0 | 0 | 0 | 0 | 0 | 1 |
| ledger incident | 0 | 0 | 0 | 0 | 0 | 14 |
| ledger show | 0 | 0 | 0 | 0 | 0 | 1 |
| ledger tempo | 0 | 1 | 0 | 2 | 0 | 3 |
| loops check | 0 | 1 | 0 | 0 | 0 | 25 |
| main-red | 0 | 0 | 0 | 0 | 0 | 2 |
| merge-driver continue | 0 | 0 | 0 | 0 | 0 | 3 |
| merge-driver set | 0 | 0 | 0 | 0 | 0 | 3 |
| nexus-ram record | 0 | 1 | 0 | 0 | 0 | 3 |
| nexus-ram report | 0 | 0 | 0 | 0 | 0 | 1 |
| nix explain | 0 | 0 | 0 | 0 | 0 | 16 |
| nix gc | 0 | 0 | 0 | 0 | 0 | 4 |
| ops credentials | 0 | 0 | 0 | 0 | 0 | 10 |
| ops dividend | 0 | 0 | 0 | 0 | 0 | 8 |
| ops github | 0 | 0 | 0 | 0 | 0 | 2 |
| ops strategy | 0 | 1 | 0 | 0 | 0 | 22 |
| ops watch | 0 | 1 | 0 | 2 | 0 | 9 |
| outside-watch run | 0 | 1 | 0 | 0 | 0 | 0 |
| outside-watch status | 0 | 0 | 0 | 0 | 0 | 0 |
| pin-retire | 0 | 0 | 0 | 2 | 0 | 0 |
| placement-survey keep | 0 | 0 | 0 | 0 | 0 | 0 |
| policy-replay case | 0 | 0 | 0 | 2 | 0 | 7 |
| policy-replay promote | 0 | 0 | 0 | 0 | 0 | 1 |
| private-check | 0 | 0 | 0 | 0 | 0 | 3 |
| provider-capacity events | 0 | 0 | 0 | 0 | 0 | 0 |
| provider-capacity status | 0 | 0 | 0 | 0 | 0 | 0 |
| red-main-notify | 0 | 0 | 0 | 0 | 0 | 4 |
| remote-serve | 0 | 4 | 0 | 0 | 0 | 5 |
| remote-serve audit | 0 | 0 | 0 | 0 | 0 | 2 |
| resident-cap gate | 0 | 0 | 0 | 0 | 0 | 2 |
| resident-cap status | 0 | 0 | 0 | 0 | 0 | 1 |
| retain | 0 | 1 | 0 | 0 | 0 | 5 |
| runtime agents | 0 | 1 | 0 | 0 | 0 | 1 |
| runtime agy | 0 | 0 | 0 | 0 | 2 | 0 |
| runtime build-ahead | 0 | 0 | 0 | 0 | 0 | 3 |
| runtime claude | 0 | 0 | 0 | 0 | 0 | 1 |
| runtime deploy | 0 | 1 | 0 | 0 | 0 | 1 |
| runtime eval-unit | 0 | 0 | 0 | 0 | 0 | 0 |
| runtime hold | 0 | 0 | 0 | 0 | 0 | 1 |
| runtime host-op | 0 | 0 | 0 | 0 | 0 | 1 |
| runtime mirror | 0 | 0 | 0 | 0 | 0 | 3 |
| runtime opencode | 0 | 1 | 0 | 0 | 0 | 0 |
| runtime order | 0 | 0 | 0 | 0 | 0 | 4 |
| runtime pi | 0 | 1 | 0 | 0 | 0 | 0 |
| runtime settle | 0 | 0 | 0 | 0 | 0 | 5 |
| runtime update | 0 | 1 | 0 | 1 | 2 | 14 |
| session-host | 0 | 0 | 0 | 0 | 0 | 1 |
| session-link | 0 | 0 | 0 | 0 | 0 | 0 |
| session-unit | 0 | 0 | 0 | 0 | 0 | 2 |
| skill-prune | 0 | 1 | 0 | 0 | 0 | 1 |
| spare-fill | 0 | 0 | 0 | 0 | 0 | 0 |
| spare-retire | 0 | 0 | 0 | 0 | 0 | 0 |
| spend | 0 | 0 | 0 | 0 | 0 | 4 |
| ssh-argv | 0 | 0 | 0 | 0 | 0 | 0 |
| ssh-master probe | 0 | 1 | 0 | 0 | 0 | 0 |
| ssh-session | 0 | 0 | 0 | 0 | 2 | 2 |
| ssh-spare hold | 0 | 0 | 0 | 0 | 0 | 0 |
| stale-revert | 0 | 0 | 0 | 0 | 0 | 2 |
| supply-chain-scan | 0 | 1 | 0 | 0 | 0 | 2 |
| test coverage | 0 | 1 | 0 | 0 | 0 | 21 |
| tool-mine | 0 | 1 | 0 | 0 | 0 | 2 |
| usage-web-read | 0 | 0 | 0 | 0 | 0 | 0 |
| work claim | 0 | 0 | 0 | 0 | 0 | 1 |
| work close | 0 | 0 | 0 | 0 | 0 | 0 |
| work list | 0 | 0 | 0 | 0 | 0 | 3 |
| work need | 0 | 0 | 0 | 0 | 0 | 0 |
| work own | 0 | 0 | 0 | 0 | 0 | 1 |
| work release | 0 | 0 | 0 | 0 | 0 | 0 |
| work review | 0 | 0 | 0 | 0 | 0 | 2 |
| work search | 0 | 0 | 0 | 0 | 0 | 1 |
| work split | 0 | 0 | 0 | 0 | 0 | 1 |
| work unneed | 0 | 0 | 0 | 0 | 0 | 0 |


Zero 30-day invocations and zero textual callers, yet not listed above because code or git calls them: `git-credential get|erase|store` (git's credential helper), `landed-tree` (north-lanes gc evidence), `session-link` (north-fleet remote placement), `spare-fill` (north-fleet-supervisor and north-runtime sync), `contract-probe` (north-runtime vendor runtime check), `runtime eval-unit` (worker sandbox request), `usage-web-read` (timer dispatch in north-timers), `help`. A zero is a candidate only; the 2026-11-10 rule that zero use alone never retires a command stands (docs/rationale/command-usage-2026-11.md).
