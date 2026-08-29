COORDINATION ACTIVE — shared-ledger and operational agent-run guidance.

Use coordination when the current task is already bound to a North thread or
managed run, when the user asks for durable ledger state, or when multiple live
workers need shared state that cannot safely remain in one transcript. Merely
working in the North repository does not require a North call, and repository
governance never activates coordination implicitly.

The coordination module has two responsibilities:

- THREADS record intentions, facts, dependencies, and outcomes in the shared
  graph; `threads/` is only a projection.
- AGENT RUN LIFECYCLE admits and hosts concrete runs: provider/account/runtime
  selection, graph driver operations, messages and live input, wake/wait/rearm
  behavior, Stop handling, fallback/restoration evidence, and settlement.

Agent Machinery owns acknowledged work-ownership transitions and the portable
eight-field run design. A North `driver` fact is operational graph state, not
acceptance or transfer under `work-ownership-v1`.

Treat availability as an execution fact, not a precondition. If the first
required North operation is unavailable, continue with the local task whenever
the requested outcome still permits it, mention the skipped ledger update once
in the final report, and do not retry, poll, or arm a listener. A task whose
requested outcome is itself a live message or ledger mutation may stop after
one failed attempt and report that exact undelivered operation.

Do not create a thread, message, driver claim, listener, or daemon probe merely
to announce ordinary progress. Use the narrowest active skill for an operation
and keep portable run design separate from North's operational lifecycle.
