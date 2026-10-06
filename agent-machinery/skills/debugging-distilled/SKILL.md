---
name: debugging-distilled
description: >-
  Diagnose and fix a non-trivial bug, regression, crash, or unexplained behavior by researching the symptom, isolating one variable at a time, and diffing good against bad runs.
---

# Debugging

Debug with evidence, not guesses. State the symptom as one observable claim:
the exact error text or behavior, where it occurs, and the closest case that
works. A working and a failing case side by side is the most valuable fact in
the investigation; find one early.

## Search before solving

Someone has usually hit the same failure. Do not solve from first principles as
if the Internet did not exist. Before the second fix attempt, and before the
first on an unfamiliar system, run a short search pass:

- the exact error string, quoted, plus product, version and platform;
- the symptom in plain words plus the component and the path that triggers it;
- issue trackers, changelogs, forums and Q&A sites, user or modding
  communities, and the upstream source or tests that emit the message.

End the pass when it yields a known cause, workaround or diagnostic technique,
or after a handful of queries find nothing relevant. A hit is a hypothesis to
test, not a fix to apply blindly. Keep the useful links in the work notes.

## Reproduce and isolate

- Make the failure reproduce on demand by the shortest command or path; note
  its rate when intermittent.
- List every difference between the good and bad case: inputs, launcher,
  arguments, environment, working directory, user, versions, files, timing.
- Change exactly one variable per experiment and predict the result before
  running it. An experiment that changes two things cannot attribute its result.
- Prefer the experiment that splits the remaining candidates in half; bisect
  commits, inputs or configuration when the list is long.

## Diff to the first divergence

Instrument the good and bad runs identically, capture to files, normalize
timestamps, PIDs and addresses, and diff. The first divergence is the lead;
later differences are usually its consequences.

Reuse instrumentation the system already has before building any: its logs,
debug flags, documented control or status APIs, sockets and test harnesses.
Prefer structured state over screenshots or OCR.

Observation tools, cheapest first:

- Logs: verbose/debug options, log files, `journalctl`, `diff -u` of
  normalized logs.
- System calls: `strace -f -tt -o run.log` filtered to `file`, `process` or
  `network`; compare opened paths and `ENOENT`/`EACCES` results. `ltrace` for
  library calls; `LD_DEBUG=libs` for loading.
- Wine and Windows programs: `WINEDEBUG=+file,+module,+loaddll`, with `+relay`
  narrowed by `RelayInclude`; compare prefix, DLL overrides and drive mappings.
  Process Monitor is the native-Windows equivalent.
- Network and IPC: `ss -tupn`, `tcpdump`, WebSocket or HTTP capture through
  devtools or a logging proxy; diff message sequences.
- Process state: `/proc/<pid>/{cmdline,environ,cwd,fd,maps}`, `lsof`,
  `gdb` backtraces and breakpoints, core dumps.
- Binaries: `strings`, disassemblers, and Frida or `LD_PRELOAD` hooks, only
  where inspecting that program is authorized.

## Fix and confirm

Fix the owning cause the divergence identifies with the smallest change.
Confirm with one run of the failing case and one of the good case. Report a
workaround as a mitigation and keep the underlying cause recorded.

## Stop rules

- Make no fix attempt without a hypothesis that an observation supports.
- After two failed fixes, stop guessing: run the search pass and a good/bad
  trace before a third. If they leave no supported hypothesis, bring one
  recommendation.
- When the evidence points outside your authority, keep the concrete
  counterexample and name who can resolve it.

Bad: relaunching a program for hours with different flag and file combinations,
changing two at once and judging each attempt by screenshot. Good: search the
exact error, run the working and failing launch paths under the same trace,
diff them, and test the first divergence by changing one variable.

Full notes: [rationale and originating incident](references/notes.md).
