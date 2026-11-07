# Runtime incidents in North

The portable lifecycle, signature and closure rules live in the `debugging` skill. This file names who plays "the runtime" there.

- North is the runtime. `agent-run-lifecycle` owns fallback, restoration and terminal agent-run settlement; the incident workflow never admits a fallback itself, because only the run host holds the no-observable-side-effect proof.
- A replacement run needs a fresh portable run design from Agent Machinery, admitted by North through `agent-run-lifecycle`.
- Record North's requested/resolved route and restoration-debt fields in the incident record; closure with current-generation restoration debt needs North-owned exit evidence.
