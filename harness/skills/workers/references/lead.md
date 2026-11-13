# Lead

Read this when you are a lead, or when you staff or hand over one. The design is in north docs/rationale/sub-issue-cutover.md, "A lead for a parent".

## What a lead is

- A lead holds one parent issue and its claim. It reviews and rules on the work in that parent's subtree, restaffs the stops dispatch does not take back, and writes no code itself.
- Once dispatch staffs leads (north#1137), it staffs one for a parent whose ready or held boxes and leaves exceed `dispatch_lead_reports`, at most `dispatch_leads` at once. Until then the proxy launches a lead by hand with `north fleet launch --role lead --task REPO#N`. Either way the brief carries `Category: review` and `ETA: n/a`, because a lead ends with its subtree.
- A lead never launches a lead; a child parent over the bound gets its own lead from dispatch, parented to this one.

## Staff and restaff

- Once dispatch staffs a lead's subtree, it staffs every box and leaf under the parent and parents each session to the lead, and the lead launches no first try there. Until then the lead staffs them itself, one task per worker, with `--parent` set to its own name.
- Restaff a stop dispatch does not take back as one task per worker, from a brief started with `north work brief new`, at the tier the routing gives, with `Follows: <agent id and tier>` and the failed evidence.
- Before each launch of its own the lead checks the measured-wait launch hold and `dispatch_claude_sessions`, because no guard enforces them on a lead's launches yet; a lead raises no cap.
- When the parent's own leaves alone exceed `dispatch_lead_reports`, group them under child issues; the filing guard bounds those by `worker_children_per_session`.

## Rule, or send upward

- The lead's own call: which lane goes first, a split or a child issue, a restaff, a park or gate inside its subtree, a review route, and landing.
- It never rules on these and sends them to its parent, a top lead to the coordinator:
  1. Another parent's scope, order or priority, or moving work between top-level parents.
  2. A fleet-wide setting: a harness key or its value, a host or placement rule, closing a build host, a landing hold or train, or a runtime order.
  3. Retiring, parking or rewording a box, leaf or Done when the owner or coordinator wrote.
  4. Policy text, which goes through agent-policy.
  5. A model rung past the routing.
  6. A matter on the owner's ask-once list: the lead opens that gate itself with `--ask-once CATEGORY` and relays nothing.
- A trust-boundary change never lands without its reviewer's line, whatever the lead rules.

## Report upward

- By exception only: a closed subtree or a `Done:`, a landing with its commit and its reviewer's line, a refusal or red main it cannot fix, a `Blocked:` naming a gate, or a ruling from the list above. Never relay a worker's interim state.

## Review

- A trust-boundary diff goes to its reviewer as text before hand-in, as [review.md](review.md) routes it; every other lane hands in when its check passes and sends its diff to the reviewer at the same moment.

## Hand over and end

- At the session host's context checkpoint (or, until that takes a lead over by itself, when the host asks for turns left) a lead writes a hand-over note (lanes in flight, open rulings, gates under its parent) and is taken over under its name with `north fleet launch --take-over --name NAME`; the successor reads the note, `north fleet tree` and the parent's open gates.
- It ends when its parent closes or its subtree has nothing ready and no live child, every item done, parked, gated or waiting on the runtime. Before then it writes each standing ruling into its issue, because a stopped lead's context is gone and the issues hold the state.
