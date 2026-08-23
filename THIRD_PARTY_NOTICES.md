# Third-Party Notices

## Caveman

North's managed response-compression adapter includes behavior adapted from
`tompassarelli/caveman`, specifically the intensity filtering semantics in
`src/hooks/caveman-subagent.js`. The adapter loads the fork's committed
`skills/caveman/SKILL.md` artifact; North does not vendor the skill content.

Upstream project: <https://github.com/tompassarelli/caveman>

MIT License

Copyright (c) 2026 Julius Brussee

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.

## Three.js Skills

North vendors skill documentation from `CloudAI-X/threejs-skills` under
`profiles/tom/skills/threejs-*`.

Upstream project: <https://github.com/CloudAI-X/threejs-skills>

Upstream commit: `b1c623076c661fc9b03dac19292e825a5d106823`

The upstream README labels the collection “MIT License” but the tracked tree at
this commit contains no standalone license or copyright notice. North treats
the imported material as MIT-licensed under its local default.

## Planning Skill Bundle

North adapts the `planning` and `prior-art` skill documentation from the
user-provided local bundle at `~/planning-skill-bundle`.

The source is a non-Git directory with no upstream URL, revision metadata,
license, or notice artifact. North preserves its local identity through the
original file hashes:

- `planning/SKILL.md`: `166e6d194790822b17b4c9e9f8020f71d88f66dedcaf907aefefa316fe8df74e`
- `prior-art/SKILL.md`: `a19a331eaf1e8c71e52c2d4ed99b09bc19aa127f3a103681139d8908344cd277`

North treats the imported material as MIT-licensed under its local default.
This is not a claim about an upstream license.

## Stewardship Skills Archive

North adapts the `program-craftsmanship`, `production-hardening`, and
`program-stewardship` skill documentation from the user-provided local archive
at `~/stewardship-skills.zip`.

The archive is a non-Git payload with no upstream URL, revision metadata,
license, or notice artifact. Its SHA-256 is
`c1ab47ba221a8a5d3f18ae310b19a0ae752901023504ee119c1d15da81d72a27`.
North preserves the original skill identities through these file hashes:

- `program-craftsmanship/SKILL.md`: `c5480a5c829e550527df6675cb77db92f6f30b72b7ca7883b29a2365d9a94325`
- `production-hardening/SKILL.md`: `e20906aa338c5ab319bcbaca8b881d5659a389ba0329df3fb2dba6b505541c93`
- `program-stewardship/SKILL.md`: `33465c8e86ac344ac522cfbf8af0f0ca058bda5c78321887166b217038123b6c`

North treats the imported material as MIT-licensed under its local default.
This is not a claim about an upstream license.
