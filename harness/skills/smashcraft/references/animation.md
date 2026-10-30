# Smashcraft animation

Read this to author or improve fighter animation: readable silhouettes, drills, locomotion, rolls, get-up attacks, grabs and throws, nine-way damage reactions, for quality and action coverage. Read smashcraft:docs/fighter-animation-work.md first. An animation repair never retunes combat timing or collision, because those belong to [character creation](character-creation.md) and their fixtures.

The deliverable is a fighter whose action reads at the gameplay side-view zoom, in both facings. A mapped clip, a rotating effect or a nonzero vertex delta does not prove that it reads. Keep private art, reference stills and recordings under `~/.local/share/smashcraft-animation-reference/` or in the private build-input store, outside Git, because they are private or licensed inputs.
## Author the action

Block three silhouettes: preparation, contact and recovery. Read each one with
particles hidden. Separate the active hand, foot or weapon from the torso, and
move the shoulders, pelvis and weight with it. Keep the weapon attached and the
fighter's proportions recognizable. A heavy swing plants a support foot and
follows through; an agile action coils and springs. Fit the phases to the
existing simulation frames. Keep a fast move fast, and add no anticipation
frames for looks.

Judge the action at the smallest gameplay view, in both facings, and judge its
key intermediate poses. Exaggerate it enough to survive the camera, occlusion
and its short duration. Inspect the rendered mesh, not bone angles: a large
rotation can still read as a static silhouette. Sources are in
[animation library](animation-library.md).

## Action families

- **Locomotion:** plant each support foot, transfer weight, alternate the
  stride and match its cadence to simulated travel. Turns and brakes make the
  intended direction clear. Jump squat compresses before it extends. Airborne
  and landing poses tell rising, falling and impact apart. A floating fighter
  moves its body, cloth or weapon instead of its feet. Ground gait clips play
  at body speed over the model's drawn stride, capped at 5x
  (GROUND_LOCOMOTION_RATE_CAP): past it the body runs ahead of the feet, which
  reads on a floater and slides on a walker. Give a grounded walker whose run
  slides at the cap a faster stock sequence if its model has one, else list it
  for an authored run.
- **Drills and spins:** turn the visible body through the attack, led by a foot
  or blade, with the pelvis and shoulders turning together. Show preparation,
  sustained rotation and a deliberate exit. A downward drill has a clear
  vertical axis and a narrow leading silhouette; a horizontal spinning cut has
  a broad arc. Symmetric weapons and straight bodies hide rotation, so vary the
  shoulder, knee and silhouette phases. An effect orbiting a still body
  fails.
- **Rolls, dodges, techs and get-ups:** show direction through the body's own
  motion, not through stage travel. Start low, tuck or rotate, then recover
  support. Keep feet and body on the floor, never below it. A get-up attack
  strikes every direction its hit regions cover, then stands. Make spot dodge,
  air dodge, missed tech, successful tech and ledge action distinct. Reuse the
  existing recovery clips before authoring new ones.
- **Grabs and throws:** author holder and victim together. Reach into the
  grip, hold stably, and show a localized pummel and a distinct release in each
  of the four directions. The victim follows through in its own pose. Keep the
  two silhouettes separate, and check pairs of unlike height and a mirror pair
  in both facings. A throw that is the unchanged hold plus a launch fails.
- **Attacks and specials:** the active limb or weapon follows the strike
  direction at the contact frames. Each aerial and each ground or air special
  needs deliberate startup, action, and recovery or landing. Charge and release
  are different poses. Keep each fighter's own body mechanics.

## Damage and shield feedback

Give each fighter high, mid and low contact-height reactions at small, medium
and large intensity: nine distinct poses. Classify a hit from the accepted
contact and its authored intensity, not from a bone query. A high hit recoils
the head and chest; a mid hit contracts or twists the torso; a low hit takes
away lower-body support. Scale the whole silhouette and the limb response with
intensity, and keep each height recognizable. Never replace the nine poses with
nine scalar root tilts.

During hitstop, blend from the actual current pose into a readable pain
silhouette, never through idle. Choose timing that suits the move: Tom
delegated it in #181, and his earlier 1–2 frames are guidance, not a gate.
Sakurai's article uses four transition frames; that is not a claimed Melee
constant. Hold the pose through the stop, then continue hitstun or tumble as
the simulation says. Presentation never advances the attack, hitstun, input or
collision clocks.

Hitstop shake is presentation only: horizontal on the ground, vertical in the
air, decaying, and compensated for camera distance. Collision stays put. Shield
contact has its own impact and defender reaction; it is not a damaging hit.
Hitlag, released hitstun and released shieldstun are distinct intervals. Body
hue during them is unresolved, so capture consecutive contact, stop and release
frames before claiming a match.

## Make the change and ship

1. Author in private output with the family's generator ([animation tools](animation-tools.md)). Materialize model-input symlinks before changing them, and keep the old indices, because live families are shared by every build.
2. Before landing an authored attack, run `bun wisp anim score --fighter F --assets PRIVATE_ASSETS` from smashcraft:ts/. Each changed move needs a passing row on all five lines, including five scores of at least 4/5 from a fresh independent judge. The author never judges their own work, because authors read intent into their poses. Judges are bare model calls through `bun wisp anim judge run` (smashcraft#557), never agent sessions, per smashcraft:docs/animation-scorecard.md. A move gets two takes at most: when its second take fails the scorecard, stop, land nothing for that move, and report `Split:` with the contact-sheet path, a sheet in the private work directory holding the six judged key poses of each take at match-camera framing, because Tom's eye is the real judge and model judges have passed broken clips. When Tom calls a shipped animation broken or accepts one, the proxy adds that clip and his verdict to the judge calibration labels. Keep a change only if its score rises and no line falls (#355). Add a missing move to the sampler before claiming that move passes. `bun wisp view motion --assets PRIVATE_ASSETS` flags dead poses in movement and recovery. Check the action once in the game, in both facings.
3. Publish under the shared shipping rules in [SKILL.md](../SKILL.md) and regenerate the asset metadata with the existing tool. Balance changes and releases follow [animation review and release](animation-review-and-release.md).
