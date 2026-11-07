# Runtime directory writes: one lock per sha

On 2026-11-07 at 17:15:40Z nexus's `north runtime update` reused the live binaries for the docs-only 9dc5d445 and was part-way through exporting the tree when the laptop's staging push of the same sha ran its install on nexus. The install moved `runtimes/SHA` aside, the follow's rename of `tree.partial` failed with `No such file or directory`, the install's cleanup of `SHA.old` failed against files still being written, and nexus held that sha back for an hour (north#822). Reuse itself opens only the programs the live runtime already has, so a runtime that was synced and not built here is not the cause.

## Decision

Everything that writes `runtimes/SHA` (reuse, build, peer pull, tree export) holds `runtimes/SHA.lock`, and the install script that moves a shipped runtime into place takes the same lock first, waiting 15 seconds. Whichever comes second finds the directory whole and leaves it, or replaces a half-made one. The update lock alone could not do this: it never covered the shipped install, and a per-host lock would make a build of one sha block the shipping of another. Collection removes a lock with its runtime.
