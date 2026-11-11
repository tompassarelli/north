# The holdlist: what north must not lose, each with a guard

In the week before 2026-11-11, 67 reverts and repeated incidents came back after their fix, because nothing stopped a later lane from removing the guard that fix had added (north#1127).

## What a hold is

A hold is one guarantee north keeps, written as a `[[hold]]` in `harness/holdlist.toml`: an `id`, `what` it promises, the `metric` that measures it, the `budget` the metric may reach, and the guard that keeps it (`guard_kind`, `guard`).
`state` is `live` when the guard is in the tree and `missing` when it is still to be written; a missing guard carries `accepted_risk`, a date and a reason.
A `[[hold.also]]` names a further guard of the same hold, and `incidents` lists the issues where the hold broke.

## How a guard is resolved

`north land scan holdlist-guards-resolve BASE..HEAD`, a land check on every lane, reads the holdlist at the lane's head and looks each live guard up in that tree:

- `test`: a `fn NAME(` in a tracked Rust file.
- `concern`: a `[[concern]]` called NAME in `concerns.toml`.
- `eval`: a file whose name before its extension is NAME under `harness/evals/`.
- `land-check`: a `land.check` or `land.after` line of `.safe-push` that names NAME.

The scan names each hold whose live guard is gone, and each hold a lane removes or turns from live to missing without saying why.
It is listed as `holdlist-guards-resolve` in `harness/gates.toml` and starts in shadow like every new gate: it logs what it would refuse until a reviewer has read those decisions and switches it to enforce. On north's tree it takes about 0.15 s (0.34 s measured with the 0.21 s lane wrapper).

## Adding or retiring a hold

Add a `[[hold]]` with the next id. Write the guard in the same lane and mark it live, or mark it missing with `accepted_risk`.
To retire a hold, or to replace or drop a live guard, put a `Holdlist: REASON` trailer in the commit message; renaming a guarded test needs the trailer too, with the holdlist row renamed in the same lane.

## The ratchet

A budget only tightens. When an improvement is confirmed, the budget becomes the new measured value plus its noise band, so the next regression past that band shows as a broken hold instead of a return to an old normal.
