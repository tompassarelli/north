Not done: bd-844-ledger has committed its fix but not landed it.

- **The fix:** the host's records store now resolves under `$XDG_STATE_HOME` when that is set, so a test stays inside its own state root. The lane is n844-host-store (6e223486b1), and its push is still running.
- **Untested:** the worker couldn't compile or run the new code first, because capacity admission refused the build on both hosts. The landing checks will build it first.
- **Still to do before it closes north#844:**
  - Re-read the stray-record count.
  - Show that count still at 16 after a full workspace test run, as the box requires.
  - Tick the three clean-up boxes.
- **Worker's Stop line:** it wrote `wait until 2026-11-08T03:30Z`, which is 10:30 AM local. It has its own watch on the push, so I'm treating that line as a mistake rather than a real wait.
- **Other workers:** bd-721-a and both north#838 workers are running, and bd-721-b and c are next in the launch queue.

Stop: landing
