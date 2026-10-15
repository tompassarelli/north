---
name: game-design-prototyping
description: >-
  Design and build playable game prototypes and worlds using Tom's local Quaternius models, scenery, and animation library; make navigation and combat legible without placeholder actors.
grounded: 2026-10-15
written: 2026-10-15
metadata:
  kind: playbook
---

# Game design and prototyping

## Scope and assets

- Use the project's design and engine to build the smallest playable route or encounter that tests its intended player decision, because a small slice shows whether the decision works before art effort is spent.
- Inspect the Quaternius library before creating actor or scenery visuals or seeking downloads, because suitable models and animations are already licensed and local; archive names, extraction and license steps are in [Quaternius library](references/quaternius-assets.md).
- Use suitable library models and animations for visible actors and available scenery, so prototypes read as a game and not as placeholder shapes.
- Reserve primitives for invisible collision, navigation debugging, terrain, deliberate attack/selection graphics or a requested geometry blockout, because those are the cases where a primitive is the right visual.
- When the library lacks a suitable model, name the concrete asset gap and choose deliberate art, so a gap is a decision and not a silent fallback.
- Extract only the chosen coherent subset into the project's assets, keep the shared archives intact and outside game repositories and web releases, and preserve pack names and member paths in the attribution record, because the pack terms (checked against the all-in-one root License.txt, including Patreon-labelled content) must stay traceable.

## World legibility

- Compose recognizable hubs, forest edges, clear paths, landmarks, useful clearings and a way home with compatible scale, materials and silhouettes, so players can navigate by sight.
- Keep scenery out of combat sightlines and leave clear ground for attack warnings, because hidden telegraphs make hits feel unfair.
- Align visible obstacles and paths with movement and collision, and align the normal camera, map, objectives and world markers with the same geography, because mismatches make players distrust what they see.
- Distinguish players, enemies, targets and damage areas at the actual viewport size, and keep nearby enemies individually identifiable without covering the world in panels, because legibility must hold at play size.
- Place labels away from bodies and action, without oversized foreground text or overlapping map text, so labels never hide what they label.
- Preserve readability when adding decorative density, fog or darkness, because atmosphere must not cost the player's information.

## Creatures and animation

- Choose creatures with usable authored idle, locomotion, preparation, attack, recovery, hit and defeat clips as the encounter requires, because the missing phase is the one players read.
- Verify clip names and skeleton compatibility, and use compatible native clips before retargeting, because retargeting adds distortion and cost.
- Let gameplay own displacement unless the movement model explicitly uses root motion, so animation never moves a character away from the simulation.
- Show impending attacks, their committed target or area and the available response through visible body animation, and align animation, warnings, damage, defense and recovery with gameplay timing, so players can react to what they see.

## Done

- Run the existing browser or game journey through navigation, enemy identification, warning, response and outcome, and inspect the normal camera and animation playback before declaring readiness, because a build that passes without being watched can still be unreadable.
- Report missing art or unobserved behavior plainly, so no one assumes it was checked.
