---
name: babashka-development
description: >-
  Develop Babashka scripts, tasks, runtime upgrades, and C interop using the consumer's runtime and current official capabilities.
---

# Babashka development

1. Read the consumer's `bb.edn`, source and packaging rules.
2. Check `bb --version`, `bb describe` and current official releases when capability or freshness matters.
3. Keep task graphs in `bb.edn` and reusable logic in namespaces.
4. Require a demonstrated runtime, library or performance need before using JVM Clojure.
5. Confirm platform, symbols, exact C ABI and libffi support before wrapping native memory lifetimes.
6. Exercise the smallest real FFI call on the target platform.

Read [runtime and releases](references/runtime-and-releases.md) for upgrades or [C ABI and memory](references/c-ffi.md) for FFI.
