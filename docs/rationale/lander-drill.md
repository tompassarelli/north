# Lander drill: every remedy is proven nightly against a scratch lander

`north land drill` injects each of the six stuck states the lander met on 2026-11-06 into a scratch lander every night and requires its remedy to clear it with nobody paged (north#634). Each was fixed once that day, and a remedy that is never exercised rots: a renamed signature, a moved file or a changed unit leaves the doctor's bound over and the proxy paged, and a person becomes the alarm again; the drill finds that before the state recurs on the real lander.

## The scratch lander

Each fault gets a fresh directory with its own home, state, runtime directory, git config and a `north` stand-in that only records its arguments, so incident rows and alarms a remedy's own program writes stay in the scratch. The drill runs in a child whose `XDG_STATE_HOME` is the scratch, so the decision logs and ledgers it writes stay there too. Every script a remedy runs on a place runs there under the scratch environment (`Reach::Scratch` in the doctor's facts), the full-disk fault inside the user and mount namespace that holds its tmpfs. Scratch user units are named `north-drill-<id>-<fault>.service` and stopped when the fault ends.

## Faults and the remedies they prove

- held link on a replay that reached no verdict: the doctor's promotion retry, run as `north land promote` against a scratch repository with one recorded landed round and a candidate that replays it landed; the lander link must follow current. `north runtime promote` runs this same command for the lander link after its own update steps, which a scratch lander has none of.
- full disk: `north capacity disk keep` under the scratch home, whose offload-cache eviction must bring a 64 MiB tmpfs filled to 95% by an unlocked offload slot back over 10% free.
- wedged lander unit: restarting a scratch user unit that is alive but has not written its rounds file for 20 minutes.
- check refused because its host lacks the runtime: requeueing the ticket once, while a scratch lander unit serves the queue.
- dead offload slot: ending the process group of a job that holds its slot's lock past the slot's deadline.
- red main from a flaky test: the sheriff reruns the failing test at the tip, reads it passing and reverts nothing.

## What counts as healed

The doctor's first round must read the injected bound over and apply its remedy without failing; every later round, polled every 2 seconds, must leave the proxy unpaged until the bound reads within its limit, within the doctor's patience of 5 minutes, the same line after which the doctor pages a person. The fault outside the doctor's bounds, the sheriff's, heals when the sheriff reverts nothing; the debt-ceiling fault went with the refusal it drilled (north#729, 2026-11-07).
