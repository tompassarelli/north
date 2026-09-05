# C FFI

## Capability and alternatives

Use the official [`babashka.ffi` guide](https://github.com/babashka/ffi/blob/main/doc/guide.md)
and [API](https://github.com/babashka/ffi/blob/main/API.md) for the exact current
contract. In Babashka the namespace is built in; the standalone library also
targets JVM Clojure and currently describes itself as work in progress.

FFI is a candidate when a stable C ABI directly removes a current subprocess,
pod, JNI/JNA, or unavailable-library boundary. It is not a default replacement
for working Clojure, Java interop, or process APIs. Confirm the target runtime,
operating system, architecture, shared-library availability, and exported ABI
before choosing it.

## ABI and lifetime boundary

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

## Rationale

FFI removes a transport layer but also removes process isolation: a signature
or lifetime error can crash the caller. Wrapping it once localizes that proof.
A pointer's numeric value does not establish validity, ownership, or lifetime;
copy/convert into ordinary domain values before leaving the boundary.
