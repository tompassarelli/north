# Smashcraft stage design

Read this to design, dress or judge a stage's look: backdrop, scenery, set pieces, water, sky, light and fighter contrast, in Classic and Definitive, for any stage art, composition, look issue or new stage.

## References

- Apply the rules and limits in smashcraft:docs/design/stage-art.md and smashcraft:docs/design/visual-quality.md, because they are the accepted art bar.
- Read ~/.local/share/smashcraft-stage-references/README.md and view its images before changing art, because judgment needs the reference beside the work.
- Use Nintendo reference captures only for judgment, never committing, importing or tracing them, because they are Nintendo's art.
- Choose a reference matching the stage's layout (horizon band, close set pieces, enclosed arena, large object, or floating island over vivid sky), because a mismatched reference misjudges composition.
- Inspect `00-anti-smashcraft-tomb-0.0.100.png` as the rejected empty composition, because it is the failure to avoid.

## Composition

- Fill the middle frame with scenery, close set pieces, enclosure, stage mass or vivid sky, because empty middle fields were rejected.
- Seat the deck in surrounding scenery, water, structure or a modeled underside mass, not a flat tiled texture, because a floating slab reads as unfinished.
- Make the landmark recognizable at match zoom close behind the deck, because that is the zoom players see.
- Render water as a lighter, rippled full-width surface at its actual height with submerged supports, entry splashes and visible swimming, because flat water reads as floor.
- Keep fighters and the fighting area brightest while reducing saturation and contrast with depth, and pass the existing fighter contrast rows, because fighters must read first.
- Compose platforms, deck and scenery as one place with consistent light and native Warcraft III assets first, reusing existing lighting packages, contrast rows and asset-family storage through `bun wisp inputs add`, because shared packages keep stages consistent.
- Preserve collision, blast zones, platform positions and frame cost while fitting art to geometry, because art never changes gameplay.

## Judging

- Complete Classic and Definitive scenes at both camera extremes with side platforms inside the frame and no clipped edges, and judge near and far match views at 16:9 beside the chosen reference, because the camera moves between them.
- Accept a look only from captures on Tom's client profile (3.0.0.24268, the VM's account b) as well as the pool, because 3.0.1-only fog, light and water levers fall back silently on his build.
- Capture Wisp frames, then one native LAN-pool capture, and confirm graphics-mode changes took effect by checking that Classic and Definitive frames differ, because a silent fallback looks identical.
- Measure below-horizon backdrop-only pixels against the limit in visual-quality.md, inspect grayscale frames at about 320 pixels wide for readable fighters and ledges without large empty fields, and judge composition visually as well, because contrast numbers alone pass empty scenes.
- Post before and after images beside the reference on the issue, naming the reference and rule each change serves, because review needs the comparison.
