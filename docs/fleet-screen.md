# The fleet screen against plain Claude Code

Status (2026-11-14): draft, not approved for publication. This page gives each reason to use the screen `north` opens instead of a plain Claude Code window, beside the measurement or the step of a recorded run that shows it, then what the plain window still does better and which gap boxes on north#1281 are still open. A reason that has no evidence is not on it.

The two sources are different in kind, and each reason says which it rests on:

- **The comparison page**, [plain-runner-comparison.md](rationale/plain-runner-comparison.md), with the rows in [readme/comparison.toml](readme/comparison.toml). It measures north's workers against T3 Code, a plain runner, not against the Claude Code client, so it bears on the work the screen starts and not on the screen's drawing.
- **The recorded run**, [Fleet screen 1](https://github.com/tompassarelli/north/issues/1281#issuecomment-6858508367): the installed binary opened with no arguments in a terminal driven key by key on a scratch fleet with real sessions, 168 steps and 316 frames, every key and command of the help tried. Its steps are cited as "step N". It ran on 2026-11-13, before most of the fixes named below.

## Reasons, each with its evidence

### One screen holds every session, each account's room and each decision

A client window is one conversation. In the recorded run the digest (Ctrl+T) put in one view each session with its first prompt, tokens, cost and the first line of its report, the accounts with week left, use per hour, outlook and reset, and the infra table (step 33). A gate showed its question, options, recommended and default marks and time to default (step 69), and a digit answered the selected gate (steps 74, 163 to 166).

What the run also showed there: no 5-hour window per account (step 33), a digest that did not scroll (steps 35, 139), and a worded answer that closed a gate at once (steps 70 to 72). The boxes that followed are named under "What the run found, since boxed".

### A worker starts from one form on a chosen account, model and directory

The new-session form launched a worker: visible at +6 s, its reply by +18 s (steps 24 to 26). The same form showed no account's room, no effort and no task (steps 12 to 17), which a later box addresses.

### The workers the screen starts cost less and finished sooner on small tasks, in two cells

Source: the comparison page, release runtime after the hand-in fix, 2026-11-14, tasks t01 to t05 of the load ladder, claude-opus-5-5 at medium effort in north, one build host for every arm, list-price cost. T3 Code's figures are those of its faster arm for that measure.

- **Small one-worker cell** (one valid run): north landed 5 of 5 at a median of 15 s and $0.050 per task; T3 Code with file text landed 5 of 5 at 18 s and $0.172. R, north's time over the faster arm's, is 0.79 (80 percent interval 0.71 to 0.83), and the 0.67 line is not met.
- **Small five-at-once cell** (two valid runs): north landed 10 of 10 at a median of 13.5 s and $0.054 per task; T3 Code with file text landed 10 of 10 at 21 s and $0.191. R is 0.64 (0.60 to 0.67).

Both cells are undecided: T3 Code's noise band is not measured and neither cell has its third valid run. Before the hand-in fix, on 2026-11-13, north's median in the same cells was 199 s and 197 s against T3 Code's 17 s and 24 s, and at ten at once it landed 8 of 10 (the two lost to the test setup); those rows are on the comparison page as losses. Ten and twenty at once after the fix, single real tasks on the release runtime, large tasks and the outside benchmark are unrun or unread, so this reason holds for these two cells and nothing wider. The large cells, which a general claim needs, have not run, and nothing here says north is ahead in general.

## What plain Claude Code still does better

From the recorded run:

- **It starts the coordinator.** `north proxy` starts the coordinator only as a client window and refused on the scratch fleet, so the loop cannot begin in the screen (step 28). Open: gap 1, and gap 1 placement for the run that shows it on a host with no desktop.
- **It copies and pastes anywhere.** Ctrl+Y and Ctrl+V failed where the Wayland tools are absent (steps 52, 53), a pasted two-line text arrived on one line (step 161) and tab characters in a reply were dropped (step 65).
- **It shows a reply as it is written.** The screen drew a reply only when whole, with nothing changing for 5 s after a tool result (step 38) and a half-written essay only at its interrupt (step 60).
- **Its slash commands do what they say.** `/model` to another model family left later replies invisible (steps 106 to 128); `/goal`, `/compact`, `/rename`, `/fork` and `/config` had no handler, and `/goal` ran 13 unasked turns (steps 133 to 135).
- **It keeps what was typed.** A refused launch discarded the brief (steps 20 to 23, 156); a launch settling moved the selection and sent a typed message to the wrong session (steps 129 to 132).

Not read in the run: "landed today" was seen only empty because nothing landed, and copy and image paste were tried only on a host without Wayland. The run did not compare the screen with a plain window on queued and urgent sends, interrupts, or quit and reopen, which worked (steps 37 to 43, 60, 62, 116 to 118), so none of those is claimed as a reason.

## What the run found, since boxed

Each item above is a box on [north#1281](https://github.com/tompassarelli/north/issues/1281), and gaps 2 to 11 and 13 are ticked there with a test. The run itself has not been repeated, so this page does not claim the loop now holds end to end; the "TUI ready" launch gate waits for that recorded run.

Still open on north#1281 as of 2026-11-14:

- **Fleet screen gap 1**, the coordinator starts without a client window: the scripted run on a host with no desktop.
- **Fleet screen gap 1 placement**: no seat has both a lead's rights and a host with no desktop, so the gap 1 run cannot yet be made.
- **Fleet screen gap 12**, the landing queue, waiting tickets and host load in the screen: the board half landed; the scripted run that lands one change and shows it waiting, then landed beside the host's load, is open.
- **Fleet screen look 6**, the transcript and the working line drawn like the Claude Code client.
- **TUI ready, a launch gate**, and this page's own box, Fleet screen 2.
