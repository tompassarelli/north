---
name: babashka-development-distilled
description: >-
  Develop, debug, upgrade, or assess Babashka scripts, bb.edn tasks, built-in
  capabilities, native distributions, and babashka.ffi C interop. Use when
  work invokes bb or when a current Babashka capability may replace shell,
  JVM-startup, subprocess, pod, or native-binding machinery.
---

# Babashka development

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

## Current decision-changing checkpoint

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

## C FFI

Use the official [`babashka.ffi` guide](https://github.com/babashka/ffi/blob/main/doc/guide.md)
and [API](https://github.com/babashka/ffi/blob/main/API.md) for the exact current
contract. In Babashka the namespace is built in; the standalone library also
targets JVM Clojure and currently describes itself as work in progress.

FFI is a candidate when a stable C ABI directly removes a current subprocess,
pod, JNI/JNA, or unavailable-library boundary. It is not a default replacement
for working Clojure, Java interop, or process APIs. Confirm the target runtime,
operating system, architecture, shared-library availability, and exported ABI
before choosing it.

Keep native details inside one wrapper namespace: load the intended library,
declare exact C signatures and layouts, scope native memory with arenas, and do
not expose pointers, layouts, or arena ownership to ordinary callers. A wrong
address or signature can terminate the process. Match string, callback, and
returned-pointer lifetimes to the C contract. Struct calls in native Babashka
require a build with libffi; verify `:libffi/version` in `bb describe` rather
than inferring it from the Babashka version.

Validate the smallest real call that distinguishes library loading, symbol
binding, ABI shape, and result handling on the target platform. Do not build a
generic native abstraction or cross-platform test matrix before an actual
consumer requires it.
