# Landing queue p90: where 38 minutes went on 2026-11-08, by stage

North's landing queue p90 target is 60 s. At 10:03 AM +07 on 2026-11-08 the scoreboard read 38 min over 1 h (n=21),
35 min over 3 h and 30 min over 24 h (north#902). This note records where those minutes went, by stage, and which
steps of the decided design (north#842) each stage waits on.

## Where the 1 h p90 went

Window: the 21 north landings between 9:10 and 10:10 AM +07 on 2026-11-08, read from the landings ledger and the
lander's `land.batch` and `land.judge` decision logs. Queued-to-landed p90 was 2,270 s (38 min, the scoreboard's
figure). The window held 31,866 lane-seconds of waiting, split like this:

- **Waiting for the first round that carried the lane: 6,765 s (21%).** A lane queued mid-round waits for that round
  to end unless it joins within 60 s. Rounds took a p50 of 301 s and a p90 of 433 s.
- **Rounds that carried the lane but did not land it: 15,391 s (48%).** A lane rode a p50 of 4 rounds and a p90 of
  7 before it landed.
- **The round that landed it: 5,990 s (19%).** From the round's start to its push, a p50 of 348 s.
- **Push and after-push bookkeeping: 211 s (1%).**
- **Not found in any round record: 3,507 s (11%).** All of this is one lane whose ticket name matches no `included`
  entry.

The rounds themselves, from 7:15 to 9:55 AM:

- There were 33 rounds. 13 landed at least one lane (77 lanes in all), 2 only named a culprit, 17 landed
  nothing, and the last was still running at 10:10 AM.
- 14 rounds ended with "probes carrying SUSPECTS were refused; the next round leaves each of them out in turn": two or
  more lanes stayed under suspicion, and each case cost at least one more round.
- A north round launched a p50 of 5 probes and a p90 of 29 (36 at most), and each probe fans out into about 6 offload
  jobs. With about 17 build-host slots, the probes of a wide round queued for slots.
- Probe checks since 7 AM (the `land checks` spans), by outcome:
  - green: 85, p50 133 s, p90 255 s;
  - refused (exit 1): 43, p50 186 s, p90 592 s;
  - test failure (exit 100 or 101): 63;
  - transient (exit 75): 29;
  - cancelled (exit 143): 12.

  A round lasts as long as its slowest probe, so a red probe of 10 minutes holds every green lane in that round for 10
  minutes.

Reproduce (any day and window; set DAY, FROM and TO in UTC):

```sh
D=~/.local/state/north/decisions; DAY=2026-11-08; FROM=2026-11-08T02:10; TO=2026-11-08T03:10
day() { for f in $D/$1/$DAY/*; do case $f in *.zst) zstdcat $f;; *) cat $f;; esac; done; }
day land.batch | jq -r .case.base | sort -u | while read b; do git -C ~/code/north/main cat-file -e $b 2>/dev/null && echo $b; done | jq -R . | jq -s . > /tmp/bases.json
day land.batch | jq -s --slurpfile b /tmp/bases.json '[.[] | select(.case.base as $x | $b[0] | index($x)) | {id, at, inc: .decision.included}] | sort_by(.at)' > /tmp/rounds.json
day land.judge | jq -s '[.[] | {f: .follows, at, inc: .case.included, d: (.decision | if type == "string" then . else keys[0] end)}]' > /tmp/judges.json
sqlite3 -readonly -json ~/.local/state/threads/snapshot.db "SELECT lane, strftime('%s',queued_at) q, strftime('%s',landed_at) l FROM landings WHERE repo='north' AND verdict='landed' AND landed_at >= '$FROM' AND landed_at < '$TO'" > /tmp/landed.json
jq -n --slurpfile R /tmp/rounds.json --slurpfile J /tmp/judges.json --slurpfile L /tmp/landed.json '
  ($R[0] | to_entries | map(.value + {next: $R[0][.key + 1].at})) as $rs
  | [$L[0][] | (.q | tonumber) as $q | (.l | tonumber) as $l | .lane as $n
    | ($J[0] | map(select(.d == "land" and (.inc | index($n)) and .at >= $q and .at <= $l + 5)) | last) as $win
    | ($rs | map(select(.at >= $q - 5 and .at <= $l and (.inc | index($n))))) as $rode
    | {wait: ($l - $q), first: (if $rode == [] then 0 else $rode[0].at - $q end),
       lost: ([$rode[] | select(.id != $win.f) | (.next // $l) - .at] | add // 0),
       landing: (if $win then $win.at - ($rs | map(select(.id == $win.f))[0].at) else 0 end),
       after: (if $win then $l - $win.at else 0 end), rode: ($rode | length)}]
  | def p(q): sort | .[(length - 1) * q | floor] | floor;
    {lanes: length, wait_p90: (map(.wait) | p(0.9)), wait: (map(.wait) | add | floor),
     first_round: (map(.first) | add | floor), rounds_lost: (map(.lost) | add | floor),
     landing_round: (map(.landing) | add | floor), after_push: (map(.after) | add | floor),
     rounds_ridden_p50: (map(.rode) | p(0.5)), rounds_ridden_p90: (map(.rode) | p(0.9))}'
```

Round outcomes come from `journalctl --user -u north-land@north -o cat` (`probing`, `probes carrying`, `is the
culprit` lines). Probe check durations come from the `land checks` spans in the queue's `logs/spans.jsonl`, grouped by
their `exit` attribute.

## Why rounds cost this much

Three properties of the round, not any one slow check, account for the 69% spent outside the landing round:

- **The barrier.** A round settles only when its last probe ends. Every green lane waits for the slowest red probe,
  and a lane that misses the round's 60 s join window waits for the next one.
- **Group testing against a high failure rate.** A probe of B lanes passes with probability (1-p)^B. Across north's
  queue, red lanes, transient builds and pairs that compile alone but not together left two or more suspects in 14 of
  33 rounds. Telling them apart took further rounds that landed nothing (north#842 records the same pattern at 2:00
  and 8:33 AM).
- **Queue depth feeds itself.** Lanes arrive at about 26 an hour. At a 25-minute mean wait, Little's law puts about 11
  lanes in the queue, and 20 to 37 at peaks. Rounds grow wider with depth, so they fail more often and saturate the
  build hosts' slots, which makes every probe slower.

## What each stage waits on

The design is north#842's (Design section, revised 2026-11-08), plus verdict reuse ([verdict-reuse.md](verdict-reuse.md))
and affected-crate checks (north#634). At 10:30 AM these of its steps were not live on the lander, ranked by the wait
each cuts:

- **The landing round and every round after it: 6 to 10 min per probe.** Every probe runs the whole tests/it suite.
  Affected-crate scoping narrows clippy and the crate tests, but `--test it -E kind(lib)|kind(bin)|binary_id(north::it)`
  still selects all of tests/it, because nearly every Rust lane reaches the root package. Per-crate tests/it selection
  (north#634) and verdict reuse (north#842 shadow and turn-on) cut this.
- **Rounds that land nothing: 48%.**
  - The refusal-rate stride (design step 4, `pace.rs`, landed but unwired) caps probe width during an incident.
  - Backup bets on spare slots (step 3) land the green tail without waiting for a conviction.
  - Suspects split in halves (n842-bisect, n842-groups) convict two bad lanes in one or two rounds.
- **Docs and harness lanes riding cargo rounds.** Footprint sub-queues (step 1, `subqueue.rs`, landed but unwired)
  let a lane whose changes select no Rust package land beside a red Rust lane.
- **Waiting for a round: 21%.** Every step above shortens the round each new lane waits out.

The hour after these land is measured with the command above and recorded here.
