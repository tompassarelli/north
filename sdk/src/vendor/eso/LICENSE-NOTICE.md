# LICENSE NOTICE — vendored ESO codec

Source: https://github.com/Green-PT/honey-for-devs  
Directory: `eso/` (index.js, ccr.js, SPEC.md)  
Vendored commit: 9682ea3b8e087adcdff6615010492983512f90b5  
Vendor date: 2026-07-03  

Files were converted from CommonJS (`module.exports`/`require`) to ESM
(`export`/`import`) for bun compatibility. North later strengthened `ccr.js`
with exact JSON-domain validation, marker-inclusive gain checks, protected-row
selection, verified cache round trips, corruption rejection, and a passive
cache-prefix evaluator. Those North modifications remain under North's
`MIT OR Apache-2.0` license; the original ESO material remains MIT-licensed
under the notice below.

The persistent scoped store, TTL/retrieval classifications, and explicit
canary integration in `sdk/src/ccr.ts` were informed by an interoperability
audit of Headroom CCR at commit
`9b016f2b64cb50cd50ab68711ab2abdf7d74c8ec`:
https://github.com/DanielAvdar/headroom (Apache-2.0). No Headroom source code was
copied. Headroom's Apache-2.0 license is preserved in that upstream repository;
North's implementation is original and remains `MIT OR Apache-2.0`.

---

MIT License

Copyright (c) 2026 Green-PT

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
