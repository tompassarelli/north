COORDINATION ACTIVE — shared-ledger guidance for messages, threads, and
assignments.

Use coordination when the current task is already bound to a North thread or
assignment, when the user asks for durable ledger state, or when multiple live
workers need shared state that cannot safely remain in one transcript. Merely
working in the North repository does not require a North call, and repository
governance never activates coordination implicitly.

The coordination module has three independent responsibilities:

- MESSAGES deliver durable or urgent communication between participants.
- THREADS record intentions, facts, dependencies, and outcomes in the shared
  graph; `threads/` is only a projection.
- ASSIGNMENTS bind an actor to a thread. Staffing resolves a role; assignment
  records who owns this concrete piece of work.

Treat availability as an execution fact, not a precondition. If the first
required North operation is unavailable, continue with the local task whenever
the requested outcome still permits it, mention the skipped ledger update once
in the final report, and do not retry, poll, or arm a listener. A task whose
requested outcome is itself a live message or ledger mutation may stop after
one failed attempt and report that exact undelivered operation.

Do not create a thread, message, assignment, listener, or daemon probe merely
to announce ordinary progress. Use the narrowest active skill for an operation
and keep orchestration decisions separate from coordination transport.
