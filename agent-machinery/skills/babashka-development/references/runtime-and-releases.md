# Runtime and release decisions

## Runtime authority

Treat the installed runtime, the consumer's `bb.edn`, and official Babashka
sources as authority. Preserve the consumer's source, packaging, and validation
rules.

## Establish current capability

Run `bb --version` and `bb describe`; the latter reports compiled features and,
on FFI-capable builds, libffi availability. When version or capability affects
the decision, check the official [latest release](https://api.github.com/repos/babashka/babashka/releases/latest),
read the relevant range in the [changelog](https://github.com/babashka/babashka/blob/master/CHANGELOG.md),
and consult the [Babashka book](https://book.babashka.org/) or the owning
repository under the [Babashka organization](https://github.com/babashka).
Do this in the current turn; never call a remembered version “latest.” Treat
social posts as discovery, not authority.

Read only the releases between the installed and target versions. Extract
changes that alter this consumer: task/dependency semantics, built-in namespaces
and classes, feature flags, platform artifacts and link modes, minimum platform
versions, or dependency/security fixes. Do not turn the changelog into a local
manual.

Prefer `bb.edn` for task graphs, paths, dependencies, pods, and CLI contracts;
keep reusable logic in namespaces rather than shell strings. Use Babashka for
fast-start scripting and orchestration when its actual runtime supports the
needed semantics. Escalate to JVM Clojure or another boundary only for a
demonstrated missing capability, library/runtime requirement, or material
performance constraint.

## Historical capability checkpoint

Verified 2026-08-31: [Babashka v1.13.220](https://github.com/babashka/babashka/releases/tag/v1.13.220)
adds experimental built-in `babashka.ffi`, revises `babashka.tasks` option and
dependency inheritance, improves Java interop call sites, and changes Linux
binary selection/linking. Before editing tasks that combine `:depends`, `:cli`,
`:exec-args`, `:exec-fn`, or `:cmd`, read that release's task semantics. Before
packaging or upgrading Linux consumers, use the release's current static versus
dynamic selection rules rather than assuming the older artifact shape.

This checkpoint is a discovery aid, not a freshness claim. A skill-maintenance
run updates it when an official release adds, removes, or materially changes a
capability or deployment constraint; ordinary application work reports a stale
checkpoint and continues from official sources.

## Why this boundary matters

Fast startup is useful only when the script's libraries and semantics fit the
runtime. A release number alone does not establish compiled features or ABI
support. Query installed capability before replacing an existing process or JVM
boundary; preserve the consumer's source-language authority.
