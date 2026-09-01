# Provenance

## North-v2 consolidation

North-v2 imported this complete first-party package from
`agent-machinery@672ea2f0cfe6c6323423fe7e2a89e6789435ced5` under its existing
MIT OR Apache-2.0 license. The package remains provider-independent, but
`north-v2:agent-machinery/` is now its authoritative live source. The former
standalone repository and North-v1 copy are migration inputs only.

Portable deltas that had accumulated in North-v1 through
`north@fced2d40016e63eb84fa1c99be6659c6b284dae5` were reconciled during the
same consolidation. They add the current delivery doctrine, verification and
skill-maintenance procedures, and catalogued Babashka, competitive-loop,
skill-importing, Three.js, and web-development skills. North-v1-specific
runtime and provider machinery was not imported.

This package was extracted and provider-neutralized from these public source
revisions:

- `https://github.com/tompassarelli/north.git` at
  `d538eb98c360e8bd40d2d6952ece10aa9a9af175`: coordination doctrine, role
  contracts, routing schemas and validators, staffing catalog, and portable
  procedures other than the uncleared payload-derived procedures listed below.
- `https://github.com/tompassarelli/firn.git` at
  `3eb5eec514aed7c5694d5dbb5a4fe21897e1b109`: verification, greenfield,
  Rust, terse-reporting, and skill-maintenance procedures.

At each revision, the repository root contains `LICENSE`, `LICENSE-MIT`, and
`LICENSE-APACHE`. North's `LICENSE` identifies `MIT OR Apache-2.0`; Firn's
`LICENSE` states the same choice. Both MIT texts carry `Copyright (c) 2026 Tom
Passarelli`. Neither revision contains a top-level `NOTICE` file. North does
contain `north:THIRD_PARTY_NOTICES.md`, but its Caveman adapter and Three.js
sections do not pertain to this package. Copying the full file is therefore not
required. The applicable full license texts are retained as
`agent-machinery:LICENSE-MIT` and `agent-machinery:LICENSE-APACHE`; factual
credits appear in `agent-machinery:NOTICE`.

Provider adapters, runtime coordination, hooks, personal and machine policy,
secrets, and non-portable project procedures were not imported.

## Excluded no-license source material

Lines 47-64 of `north:THIRD_PARTY_NOTICES.md` record that `planning`,
`prior-art`, and the source of `build-vs-reuse` came from the non-Git local
directory `~/planning-skill-bundle`, which had no license or notice. Lines
66-82 record that `program-craftsmanship`, `production-hardening`, and
`program-stewardship` came from the local archive
`~/stewardship-skills.zip`, also with no license or notice. North's stated
local default of treating those payloads as MIT is expressly not a claim about
their upstream license and does not itself grant copying, adaptation, or
redistribution permission.

At extraction base `bd2f7c2733abf6ea35653f8c5431e42177d195dd`, these files
contained copied or lightly edited expression from those payload-derived North
files:

- `agent-machinery:skills/planning/SKILL.md` lines 1-130
- `agent-machinery:skills/prior-art/SKILL.md` lines 1-132
- `agent-machinery:skills/build-vs-reuse/SKILL.md` lines 1-87
- `agent-machinery:skills/program-craftsmanship/SKILL.md` lines 1-87
- `agent-machinery:skills/production-hardening/SKILL.md` lines 1-92
- `agent-machinery:skills/program-stewardship/SKILL.md` lines 1-94

The North and Firn repository licenses did not clear that payload expression
for redistribution: a repository owner's local default cannot grant rights in
an unidentified payload owner's work, and attribution cannot substitute for
permission. Before release, all six package files were independently rewritten
in full. The no-license payload expression is excluded from this distribution.

North's role-contract documentation credits Julius Brussee's cavecrew agents
for inspiration. North added that credit in commit
`8de88ff77d04b758d466e5ff4b73441cbf4ea967`; the last caveman revision before
that adaptation, `686c0cce646f6f50c95e596a1ab5768ffa73cb2b`, carries an
MIT license and `Copyright (c) 2026 Julius Brussee`. North added its Matt
Pocock credit in commit `684e72bfcc0a2f7cb8df5eb84ea8eab075b4c8be`;
the last `mattpocock/skills` revision before that adaptation,
`2ee14df19c2bbea2303e0a187e8619a5ddef8817`, carries an MIT license and
`Copyright (c) 2026 Matt Pocock`. The package's composition method is an
independent replacement of North's adapted passage, and its communication
contracts do not copy cavecrew expression. The package retains the source
credits as provenance; neither third-party MIT permission notice is required
for idea-level inspiration alone.
