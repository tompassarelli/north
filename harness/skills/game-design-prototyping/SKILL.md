---
name: game-design-prototyping
description: >-
  Playable game prototypes and worlds with Tom's Quaternius models and animations.
grounded: 2026-11-10
written: 2026-11-10
metadata:
  kind: playbook
---

# Game design and prototyping

## Scope and assets

- Build the smallest playable route or encounter that tests the project's intended player decision with its design and engine, because a small slice proves the decision before art effort is spent.
- Inspect the Quaternius library before creating actor or scenery visuals or seeking downloads, because suitable models and animations are already licensed and local; [Quaternius library](references/quaternius-assets.md) holds archive names, extraction and license steps.
- Use suitable library models and animations for visible actors and available scenery, so prototypes read as a game, not placeholder shapes.
- Reserve primitives for invisible collision, navigation debugging, terrain, deliberate attack or selection graphics, or a requested geometry blockout, because a primitive is the right visual in those cases.
- When the library lacks a suitable model, name the concrete asset gap and choose deliberate art, so the gap is a decision, not a silent fallback.
- Extract only the chosen coherent subset into the project's assets and keep the shared archives intact and outside game repositories and web releases, with the attribution record and pack terms as [Quaternius library](references/quaternius-assets.md) requires, because the terms must stay traceable.

## World legibility

- Compose recognizable hubs, forest edges, clear paths, landmarks, useful clearings and a way home with compatible scale, materials and silhouettes, so players navigate by sight.
- Keep scenery out of combat sightlines and leave clear ground for attack warnings, because hidden telegraphs make hits feel unfair.
- Align visible obstacles and paths with movement and collision, and the normal camera, map, objectives and world markers with the same geography, because mismatches make players distrust what they see.
- Distinguish players, enemies, targets and damage areas at the actual viewport size, and keep nearby enemies individually identifiable without covering the world in panels, because legibility must hold at play size.
- Place labels away from bodies and action, without oversized foreground text or overlapping map text, so labels never hide what they label.
- Preserve readability when adding decorative density, fog or darkness, because atmosphere must not cost the player's information.

## Creatures and animation

- Choose creatures with usable authored idle, locomotion, preparation, attack, recovery, hit and defeat clips as the encounter requires, because the missing phase is what players read.
- Verify clip names and skeleton compatibility, and use compatible native clips before retargeting, because retargeting adds distortion and cost.
- Let gameplay own displacement unless the movement model explicitly uses root motion, so animation never moves a character away from the simulation.
- Show impending attacks, their committed target or area and the available response through visible body animation, and align animation, warnings, damage, defense and recovery with gameplay timing, so players can react to what they see.

## Done

- Run the existing browser or game journey through navigation, enemy identification, warning, response and outcome, and inspect the normal camera and animation playback before declaring readiness, because a passing build nobody watched can still be unreadable.
- Report missing art or unobserved behavior plainly, so no one assumes it was checked.
