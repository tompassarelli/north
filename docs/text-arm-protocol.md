# Text-arm protocol for graph-upstream files

A text-arm task keeps Fram's global adoption markers intact. Its de-adoption is
task-local: a short-lived token tells the composed `code-upstream-guard` to
allow text mutation only for the assigned thread and listed files. Physical
removal from `~/.config/fram/graph-upstream-files` would affect concurrent work
and is not part of this protocol.

The scoped token is a proposed guard seam, not deployed behavior. Until the
proposal in `~/code/north/main/patches/code-upstream-text-arm-token.proposal.patch`
lands through the sealed enforcement channel, the current guard has no
task-scoped exception. The existing global kill switch is too broad for a
comparable A/B run, so text-arm execution is blocked rather than silently run
with all authoring guards disabled.

## Run the arm

1. Establish eligibility and record the assignment before the first mutation:

   ```sh
   ~/code/north/main/bin/north-arm-assign --record \
     --thread <thread> <stable-task-id>
   ```

   Continue here only when it prints `text` and records `run_arm text`.

2. Have the orchestrator issue one token with a command shaped as follows (the
   issuer is part of the sealed landing, not implemented in this tree):

   ```sh
   ~/code/north/main/bin/north text-arm-token issue --thread <thread> \
     --ttl 2h --file ~/code/<repo>/path/file.bclj --why "assigned text arm"
   ```

   The default TTL is two hours and the maximum is four. The token names the
   exact thread, exact canonical files, assignment, reason, issuer, issue/expiry
   epochs, and the SHA-256 of its `text_arm_token_issued` audit fact. Bind its
   path as `NORTH_TEXT_ARM_TOKEN` and the task as `NORTH_TASK_THREAD`. The guard
   proposal rejects symlinks, unsafe modes, ownership mismatch, expiry, task
   mismatch, and paths outside
   `$XDG_RUNTIME_DIR/north/text-arm-tokens/`.

3. Edit only the token-listed files as text, run the task's ordinary acceptance
   check, and make the text-arm Git commit. A force assignment records
   `run_arm forced-text` plus `run_arm_why` and is excluded from A/B aggregates.

4. Reconcile each edited module through the live code coordinator:

   ```sh
   ~/code/fram/main/bin/fram-commit-code <module> \
     ~/code/<repo>/path/file.bclj --port <fram-code-port> \
     --log ~/code/<repo>/.fram/code.log
   ~/code/fram/main/bin/fram-render-code <module> \
     --port <fram-code-port> --log ~/code/<repo>/.fram/code.log \
     --out /tmp/text-arm-reconciled.bclj
   cmp ~/code/<repo>/path/file.bclj /tmp/text-arm-reconciled.bclj
   ```

   `fram-commit-code` is the actual incremental mechanism: it accepts one new
   module projection, derives pre-state through the coordinator, and commits
   changed AST nodes through the fenced code-log writer
   (`~/code/fram/main/bin/fram-commit-code:12`, `:97`, `:163`, `:174`).
   Fram's byte-identity fixture drives that candidate through the incremental
   commit and verifies the published post-commit render
   (`~/code/fram/main/tests/coord_edit_min_byte_identical.clj:102`, `:126`,
   `:137`).
   `fram-code-on` is not the reconciliation command; it discovers every Beagle
   source and replaces the whole code log (`~/code/fram/main/bin/fram-code-on:160`,
   `:173`, `:176`).

5. Treat a nonzero `cmp` as measured reconciliation loss, not a reason to hide
   the result. Fram's renderer normalizes source layout; its flip gate promises
   byte identity against `render(text)`, not arbitrary input bytes
   (`~/code/fram/main/tests/coord_code_flip_test.clj:11`). Replace the working
   file with the graph render, rerun the acceptance check, commit the
   reconciliation projection, and record the added retry, wall time, and changed
   bytes. If content or compilation changes, record a failed outcome and stop.

6. Record `text_arm_reconciled` with token id, module, commit, `cmp` result, and
   completion time; then delete the token. Expiry also restores refusal, but it
   does not substitute for reconciliation evidence.

The mechanism is AST-preserving but source-byte-normalizing. That lossiness is
part of the text arm's measured cost. A run without a valid token or completed
reconciliation is protocol-invalid and excluded from the A/B aggregate. The
token is a same-UID scope and audit control, not a cryptographic security
boundary; its authority comes from the orchestrator-only issuer and durable
assignment/audit facts.
