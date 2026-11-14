# The fleet screen against plain Claude Code

Status (2026-11-14): draft, not approved for publication. This page gives each reason to use the screen `north` opens instead of a plain Claude Code window, beside the step of a recorded run or the row of a comparison that shows it. Then it says what a plain window still does better, what is built but not yet shown, and what the run did not show.

## Where the evidence comes from

- **The recorded run**, [Fleet screen 1](https://github.com/tompassarelli/north/issues/1281#issuecomment-6858508367): the installed binary opened with no arguments in a 180x50 terminal that a script drove key by key, on a scratch fleet holding real sessions on a subscription login (a stand-in coordinator and four short-lived workers). It logged 168 numbered steps and 316 frames. This page cites it as "step N". The run came before most of the fixes named below, and it did not put a plain Claude Code window beside the screen.
- **The comparison**, [plain-runner-comparison.md](rationale/plain-runner-comparison.md), with its rows in [readme/comparison.toml](readme/comparison.toml). It measures north's workers against T3 Code, a plain runner, with the model held equal. It does not measure the Claude Code client, so it bears on the work the screen starts and not on the screen's drawing. The page is a draft not approved for publication, so its rows are cited as they stand, with only the numbers it states.
- **The boxes**: each gap, look and newcomer item is a box on [north#1281](https://github.com/tompassarelli/north/issues/1281), cited as "gap N", "look N" or "newcomer N". A fix that landed with no recorded run after it is stated as built, not as shown.

## Reasons to use the screen

Each reason below rests on a run step or a comparison row. Items a plain window also does are under the parity list further down.

### One list of decisions waiting on you

The digest (Ctrl+T) showed each open gate with its question, options, recommended and default marks and time to default (step 69). A digit answers the selected gate with that option (steps 74 and 163 to 166), and the coordinator received the answer (step 75). A plain window has no list of gates to answer. In the run, a worded answer closed a gate at once and the coordinator could not use it (steps 70 to 73); that is built, not shown (gap 6).

### Each account's room on one screen

The digest's accounts table showed, for each account, the week left, use per hour, outlook and reset (step 33). The table had no 5-hour window at the time (step 33), and one has been added since (gap 10, built, not shown).

### Every session with its report, in one place

The digest listed each session with its first prompt, tokens, cost and the first line of its report (step 33). A worker's report was read without opening its session, and the full transcript scrolled with PgUp and PgDn, with Esc in the list jumping back to the latest output (steps 50 and 51).

### A worker starts from one form, with its account chosen in it

The new-session form launched a worker: the session was visible at +6 s and its reply came by +18 s (steps 24 to 26). The account is picked for each session in the form (steps 13 to 17). The form's missing effort, task and box fields, and its account list without week left, are built, not shown (gap 9).

### Stopping and forgetting ask first

Ctrl+X asks before a stop: `n` or Esc keeps the session (steps 88 and 159), and `y` or Enter stops it (steps 89 and 162). Forgetting a session that is still running is refused with a pointer to stop it first (steps 101 to 103).

### Small tasks land faster, in two cells, against T3 Code

These are rows of the comparison, as they stand on its release-runtime section after the hand-in fix ([the section](rationale/plain-runner-comparison.md#release-runtime-2026-11-14-after-the-hand-in-fix)). T3 Code's figures are those of its faster arm for each measure.

- **Small tasks, one worker**: north landed 5 of 5 at a median of 15 s and $0.050 per task; T3 Code with file text landed 5 of 5 at 18 s and $0.172. R is 0.79 (80% interval 0.71 to 0.83). The cell has one valid run, its noise band is not measured, and the 0.67 line is not met, so it is undecided.
- **Small tasks, five at once**: north landed 10 of 10 at a median of 13.5 s and $0.054 per task; T3 Code with file text landed 10 of 10 at 21 s and $0.191. R is 0.64 (0.60 to 0.67). The cell has two valid runs and no noise band, so it is undecided.
- **Before the hand-in fix** the same cells were losses, as the comparison records them: median 199 s for north against 17 s at one worker, and 197 s against 24 s at five at once.

The comparison has no run after the fix at ten or twenty at once, and none of its large-task rows has run. This reason holds for the two cells above and nothing wider; nothing here says north is ahead in general. It measures how fast work lands, not the screen's keys.

## Parity: what a plain window also does, and the run showed working

These are not reasons to use the screen, because a plain window does them too. They are listed so that a reader knows the screen did not fail them in the run.

- Multi-line sends: Alt+Enter adds a line and Enter sends, echoed at +0.25 s and answered at +1.8 s (step 37). Shift+Enter adds a line and Ctrl+Enter sends urgently (steps 137 and 138).
- A message sent mid-turn was queued and delivered when the turn ended (step 41). An urgent send with Alt+U was answered at +1.7 s (step 43).
- Esc and Ctrl+C interrupted a long reply at +0.5 s and +0.3 s (steps 60 and 62).
- A reply shows its Bash call and output (step 38), and a markdown table as a table (step 39).
- Quit and reopen kept both sessions and their transcripts, and a reply missed while the screen was closed was drawn after the reopen (steps 27 to 29 and 116 to 118). A stopped session came back with `r` (step 91).
- `j`, `k`, `g`, `G`, Home, End and the arrow keys switch sessions (steps 92 to 99).

## What a plain Claude Code window still does better

- **It starts the coordinator where a window can open.** `north proxy` starts the coordinator only as a client window, and on the run's fresh fleet, on a host with no desktop, it refused (step 28). The screen can now hold the coordinator itself: legs A to D, F and H of gap 1 are on main, but the scripted run that shows a message answered in the screen on a host with no desktop is open. Gap 1 placement is also open: no seat has both a lead's rights and a host with no desktop, so that run cannot yet be made.
- **It draws the conversation the way the client does.** Look 6 is open: the client marks a user message with `>`, a reply and each notice with `●`, a tool call as `● NAME(ARGS)` with its result under `⎿`, and a working session as `✻ Working…` above the composer (look 6 on north#1281). Looks 1 to 5 are built, not shown.
- **A refusal is shown in full and named right.** A refusal longer than the form's two footer rows is cut mid-clause (newcomer 1; frames 23 and 28 of [the newcomer run](https://github.com/tompassarelli/north/issues/1281#issuecomment-6875698052)). A launch refused by a session scope failure is shown as "North couldn't read the conversation state" (newcomer 2; the same frames). Open.
- **`?` in an empty composer leaves nothing typed.** The help opens, and after Esc the composer still holds `?`, where the client keeps the prompt empty (newcomer 3; frames 11 and 33 of the same newcomer run). Open.

## Built since the run, not shown by a recorded run

Each item names the run step that showed the fault, and the commit that fixed it on main. None has a recorded run after its fix.

- Gap 2, a model change keeps the conversation in view (steps 106 to 128): 1042413bc2.
- Gap 3, a settling launch does not move the selection under typing (steps 77 and 129 to 132): 3e3cda2bf4.
- Gap 4, a reply draws while it is written, and a working session shows its elapsed time (steps 38 and 60): 5e0f6ec0ea.
- Gap 5, the sessions and the focused pane are named on the main view (steps 30 to 32 and 80 to 86): 250487556e.
- Gap 6, a worded gate answer waits for a second Enter before it is sent (steps 70 to 73): 50af3cdcba.
- Gap 7, the help lists only commands the screen has (steps 133 to 135): ee00e39390.
- Gap 8, a refused launch keeps the form as typed (steps 20 to 23 and 156): 910de1663c.
- Gap 9, the form takes effort, task and box, and lists each account's week left in placement order (steps 12 to 17, 33 and 154): 3f36463b21.
- Gap 10, the digest scrolls with PgUp and PgDn and shows each account's 5-hour window (steps 35, 139 and 141): 5daee76e90 and 76defd67fc.
- Gap 11, each digest panel row stays on one line, an amount that rounds to zero prints as $0.00, and no status line is shown for a project the fleet does not have (step 33): 27b1a88802.
- Gap 13, copy, paste and tabs work where the Wayland tools are absent (steps 52, 53, 161 and 65): 5910533502 for copy, f940c2408f for paste, 83096a59be for tab stops.
- Looks 1 to 3, plain white composer rules, no session strip under the chat, and the chat's title on the status row: ddd7da6365.
- Look 4, the coordinator's conversation drawn live in its pane: 5c1b7c256a.
- Look 5, the user's message in a tinted row and a model's thinking as dim italic paragraphs: f5b0412ea9.
- Gap 12's board half, waiting tickets and host load in the digest (north#1416): 08ffca6f0e.
- A take-over of a window-held proxy opens the provider's interactive client on its conversation (north#1427): 83293550cf.

## Not yet shown

- **The landing queue in the screen (gap 12).** Waiting tickets and host load are built in the digest (see above). The scripted run that lands one change and shows it waiting, then under landed today beside the host's load, is open. In the run, landed today was seen only empty (step 33). A plain window has no landing queue, so this is a view not yet shown, not a loss.

## What this page does not show

- The run used one host, so sessions across machines in one view are not shown. The digest's infra table was seen only on that host (step 33).
- Starting work by asking the coordinator was exercised only as far as the message, because a scratch fleet's coordinator could not open gates (step 66).
- No change landed in the run, so the landed-today list was seen only empty (step 33).
- Copy and image paste were tried only on a host without the Wayland tools (steps 52 and 53).
- Every timing on the screen is from one run in one terminal. The timings in the comparison rows come from their own runs, and those runs are not yet decided.
- The TUI-ready launch gate on north#1281 waits for a recorded run of the daily loop, which this page does not provide.
