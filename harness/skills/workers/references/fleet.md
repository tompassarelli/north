# Fleet org and take-over

Read this when you launch a child session, message one, or hand a proxy or lead to a successor.

- The proxy coordinates fleet sessions directly. Set `--parent` when launching a child; `north fleet tree` shows the org. A lead exists only for a multi-session workstream. Message a session with `north fleet send NAME TEXT...`, or with `north fleet ask NAME QUESTION...` when you need an answer.
- Hand a proxy or lead over by launching its successor through Fleet under the same name with `north fleet launch --take-over --name NAME`, which ends the holder and moves the name and every message it never took to the new session, because a successor under another name or outside Fleet leaves the old session reviving on every report sent to the name.
- A child launched with `--parent` reaches that parent as one fleet message, queued behind a busy parent, when a turn with a reply ends on `Done:`, `Not done:`, `Split:`, `Stop:` or `Blocked:`, lands a commit, or fails, so restaff on that message and never on the next heartbeat; progress, interrupted and empty turns post nothing.
- Long-running work continues in a fresh fleet session with `Follows:` and the prior session as parent.
