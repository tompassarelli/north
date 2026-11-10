# Fleet org and take-over

Read this when you launch a child session, message one, or hand a proxy or lead to a successor.

- The proxy lives in Tom's interactive window, never in a headless session: a revive, an account move or a report wake leaves it in a window Tom can type into, and a successor takes the window rather than a session no one can type into.
- The proxy coordinates fleet sessions directly. Set `--parent` when launching a child; `north fleet tree` shows the org. A lead exists only for a multi-session workstream. Message a session with `north fleet send NAME TEXT...`, or with `north fleet ask NAME QUESTION...` when you need an answer.
- Hand a proxy or lead over by launching its successor through Fleet under the same name with `north fleet launch --take-over --name NAME`, which ends the holder and moves the name and every message it never took to the new session, because a successor under another name or outside Fleet leaves the old session reviving on every report sent to the name.
- When the successor is an interactive Claude window someone already types into, run `north fleet adopt --name NAME` inside that window: it ends and archives the holder and binds the name to the window's own conversation, delivering every message the holder never took.
- Restaff a child on its one fleet message to its `--parent` (a `Done:`, `Not done:`, `Split:`, `Stop:` or `Blocked:` report, a landed commit or a failure) and never on the next heartbeat.
- Long-running work continues in a fresh fleet session with `Follows:` and the prior session as parent.
