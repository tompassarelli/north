---
name: babashka-development
description: >-
  Develop Babashka scripts, tasks, runtime upgrades, and C interop using the consumer's runtime and current official capabilities.
grounded: 2026-10-15
written: 2026-10-15
---

# Babashka development

1. Read the consumer's `bb.edn`, source and packaging rules, because they fix the runtime, dependencies and entry points you must keep.
2. Check `bb --version`, `bb describe` and current official releases when capability or freshness matters, because features and bundled libraries differ between releases.
3. Keep task graphs in `bb.edn` and reusable logic in namespaces, so tasks stay declarative and the logic stays testable and shared.
4. Require a demonstrated runtime, library or performance need before using JVM Clojure, because it costs startup time and packaging weight that a script does not otherwise pay.
5. Confirm platform, symbols, exact C ABI and libffi support before wrapping native memory lifetimes, because a wrong signature or lifetime corrupts memory instead of failing cleanly.
6. Exercise the smallest real FFI call on the target platform, because a call that works elsewhere says nothing about this ABI.

Read [runtime and releases](references/runtime-and-releases.md) for upgrades or [C ABI and memory](references/c-ffi.md) for FFI.
