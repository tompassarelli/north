---
name: babashka-development-distilled
description: >-
  Develop Babashka scripts, tasks, runtime upgrades, and C interop using the consumer's runtime and current official capabilities.
---

# Babashka development

Use the installed runtime, consumer's `bb.edn`, and official sources as
authority. Preserve the consumer's source and packaging rules.

Check `bb --version` and `bb describe` when runtime capability matters.
For an upgrade or a “latest” claim, inspect current official releases and only
the relevant changelog range. Social posts and stored checkpoints are discovery
aids, not freshness evidence.

Keep task graphs and dependencies in `bb.edn`, reusable logic in namespaces,
and host-language boundaries consistent with repository policy. Use JVM
Clojure only for a demonstrated runtime, library, or performance requirement.

For FFI, confirm the platform, library, symbols, exact C ABI, and libffi support.
Keep native memory and pointer lifetimes inside one wrapper; return ordinary
typed values to callers. Prove the smallest real call on the target platform.

Full notes: [runtime and release decisions](references/runtime-and-releases.md)
and [C ABI and memory ownership](references/c-ffi.md).
