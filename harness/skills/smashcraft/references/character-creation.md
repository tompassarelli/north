# Smashcraft character creation

Read this to design, build and test a Smashcraft fighter or move: role, move phases, hit regions, damage, knockback, shield, recovery, rollback state and selection. It is for Smashcraft only, not other Warcraft maps.

## Setup and sources

- Start from `origin/main` in a worktree with `bun install --frozen-lockfile` in ts/, because the lockfile pins the Wisp consumer.
- Query source or move data for tuning numbers, because remembered numbers drift from the code.
- Follow smashcraft:docs/gameplay-design.md and smashcraft:docs/delivery-goal.md, and keep windows within Tom's accepted gameplay-design.md bounds, because those are his accepted design.
- Author original regions and capsules while citing Melee rows only for factual timing or trade-off relations under `external-code`, because only facts, not expression, may be taken.
- Keep the fighter's coverage checklist in its issue until an agent on a VM pair can select it and play its moves under native capture, because that is when the fighter is done.
- Read [source map](character-source-map.md) when locating fighter implementation files and [moveset](character-moveset.md) when listing required actions.
- Read [design reading list](character-design-reading-list.md) for fighting-game, Melee or interaction-graph design questions.

## Brief before implementing

- Brief a fighter's role, measurable strengths and weaknesses and differences from every existing fighter, because a fighter without a distinct role duplicates another.
- Brief each move's decision for both players, reward, startup, active, recovery, landing, shield outcome and spacing risk, with original values marked provisional, because tuning changes them.
- State how the fighter launches into follow-ups such as tech chases, and its recovery reach, ledge options and edge-guard weakness, because those define its matchups.
- Give new fighters a play-style profile and cite before and after tuning scores from `bun wisp farm balance --wait`, because balance claims need measurement.
- Apply docs/design/balance.md (40-60% wins, top-move spam wins at most 45%, no move above 40% damage except a named signature) and record explosive openings per kill and archetype, aerial, approach, ranged and special shares in each fighter's design doc, because those are the roster's balance gates.

## Move design

- Diversify moves through hitboxes, startup, recovery and rewards, without stale moves, freshness bonuses or L-cancel, because those mechanics are rejected.
- Apply automatic half-authored landing lag to every aerial in sim/moves.ts, because it is a shared rule.
- Prevent grabs, throws and starters from trapping victims in input-insensitive loops, and name guaranteed strings and checked victim inputs until the escape detector lands, because an inescapable loop ends the game.
- Trade greater shield damage for later recovery or earlier defender response under docs/move-comparisons.md, because shield pressure needs a cost.
- Author startup, active, recovery, cancel and contact windows, eligibility, landing lag, intangibility, resources, cooldowns, damage, launch vector, base and growth, hitlag and shield behavior, because an unset field falls back to defaults that misrepresent the move.
- Allow one hit per target per attack in each positive contact window, because hits otherwise stack unintentionally.
- Treat frame 0 as the start tick and startup N as first active frame N, displayed N+1, because frame data uses that convention.
- Preserve shared per-action timing except `characterAttackActiveFrames`, justifying any new per-fighter startup or total path, because shared timing keeps the roster consistent.
- Match the whole-duration attack clip to timing changes, because a short clip freezes on its last pose.
- Resolve contact from authored volumes, not rendered bones or effects, and test simultaneous contacts and trades through the shared resolver, because rendering is not deterministic across clients.
- Use sim/conditions.ts's shared dodge profile unless Tom changes it, because dodge is a roster-wide rule.
- Spend the aerial jump and enter helpless fall on up-special while retaining one aerial jump after walking off a ledge, and implement computer recovery in match/botRecovery.ts, because that is the shared recovery rule and bots must recover too.

## Simulation, input and presentation

- Snapshot and reset every outcome-changing phase, buffer, charge, grab, resource, projectile, summon, stable ID and hit registry on stock loss and rematch, and keep native handles, visuals and audio outside replay, because rollback replays only simulated state.
- Give effects and sounds stable event IDs, because replays otherwise duplicate them.
- Use normalized rebindable actions with distinct jab, tilt, smash and aerial directions, jump-squat buffering, facing-relative back air, shield grab and designed special cancels, and keep C-stick down-air independent of Down input and fast-fall, because those are the control contract.
- Hold attacker and victim contact poses for their exact simulated hitlag without pausing input or match time, because presentation never advances simulation clocks.
- Present shield break as a seamless dizzy loop with shared starbursts cleared on stock loss and rematch, because leftover effects leak into the next stock.
- Check gameplay-size side-view poses in both facings for attached weapons, visible active limbs, stage-grounded knockdowns and readable ground and air reactions, because those fail most often; animation quality follows [animation](animation.md).

## Assets and selection

- Generate fighterAssetInfo.ts through `smashcraft-tools build-animation-assets` (smashcraft:tools/smashcraft-tools) and package.ts from tools/animations/ Blender clips, because the module is generated.
- Keep proprietary models under ~/.local/share/smashcraft-build-inputs/ under `repo-safety`, because they never enter Git.
- Register new model families in ts/scripts/wisp/mapInputs.ts and expectations in ts/scripts/wisp/playerView.ts, because the build and view checks read them.
- Supply roster chip, portrait, name, HUD plate, mirror match, rematch and off-screen indicator with text in the game's language, because a fighter is selectable only with all of them.
- Assign a campaign-appropriate home stage, native assets first, with a one-line lore reason in ts/src/game/menu/homeStages.ts and docs/design/home-stages.md, and satisfy ts/test/home-stages.test.ts for every selectable fighter (shared stages allowed), because that test gates selection.

## Test and land

- Edit with `bun wisp dev` and demonstrate a retained test's unique mutant kill under the `verification` skill, because a test that kills no mutant protects nothing.
- Compare `bun wisp interactions --move FIGHTER:MOVE` before and after and record changes in the commit message, because interaction changes are the reviewable effect.
- Update retained move-data fixtures under `verification` when frame data or regions change, through [character tools](character-tools.md).
- Run `bun run check` and the full `bun run test` suite, play both facings through `bun wisp hot` or `bun wisp lan fresh MAP`, then land under the shared rules in [SKILL.md](../SKILL.md) and fix forward if CI's Lua, parity or perf suites fail, because CI gates after landing.
- Read [balance and release](character-balance-and-release.md) for balance changes or release gates.
