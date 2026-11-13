# NixOS release notes, first review (2026-11-10)

Cycle record for the followed source [NixOS release notes](../../references.md#nixos-release-notes), made by the
method in [watching.md](../../../harness/skills/agent-policy/references/watching.md). The source was added on
2026-11-05 by the plan in [nix-footguns.md](../nix-footguns.md#staying-current-with-nix) (north#523); this is its
first cycle.

## What we looked at

- nixpkgs at `nixos/doc/manual/release-notes`, from `807ccdddd12f` (2026-11-03) to `68060cbcdcd1` (2026-11-10).
  Watch-review's blobless bare clone held both revisions; the directory at each was extracted with `git archive`
  into a new empty scratch directory and read as data. License at `68060cbcdcd1`: MIT (`COPYING`, copyright Eelco
  Dolstra and the Nixpkgs/NixOS contributors). Nothing was copied; this record paraphrases.
- 58 commits touch the path. 51 are merges: 44 syncs between master, staging-next and staging, and 7 merge-queue
  merges of the pull requests below. 7 carry content. The net diff is `rl-2611.section.md` only, +22 and -0 lines,
  8 entries, all in the unreleased 26.11 notes.
- Firn (nixos-config) at `19143f0b` (2026-11-09) and north at origin/main `5ba3314f8f`.

What the 8 entries say:

- GNOME is updated to 51 (highlight).
- New opt-in module `xdg.directories`: when enabled it sets `$XDG_CACHE_HOME`, `$XDG_CONFIG_HOME`,
  `$XDG_DATA_HOME` and `$XDG_STATE_HOME` to the specification's fallback values. A backward-incompatibility entry
  warns that applications which honour the variables only when they are set may then lose track of old files.
- New modules: `services.falco` (eBPF runtime security), `programs.openlogi` (Logitech HID++ companion) and the
  phrog display manager for phosh.
- LaSuite Docs moves to 6.x with a data migration; the tdarr package attributes are renamed, with deprecated
  aliases.

Reverts and flip-flops: none. Every content commit only adds lines, and no line added in the range is removed in it.

## How we evaluated

1. Each mechanism the 2026-11-05 plan put in place was checked live, with file or command evidence.
2. Each entry was checked against what north's three stamped Nix files teach (`harness/nix/cards.toml`, the
   nix-development skill, the firn skill) and against firn's modules, by `rg` counts.
3. The cost of following the source was measured by replaying watch-review's due rule over 61 days of nixpkgs
   history in the same clone.

## What the 2026-11-05 plan put in place, live or not

- Stamp: live. `harness/skills/nix-development/SKILL.md:7-8`, `harness/nix/cards.toml:2-3` and firn's
  `modules/north-profile/firn/skills/firn/SKILL.md` carry `nix-version: 2.34` and `nixpkgs-lock: 2026-11-02`, the
  date of north's `flake.lock` nixpkgs (nixos-26.05); wisp window and makgora lock the same date.
- Follow: live. Watch-review counted 58 commits on this path and launched this cycle.
- Trigger: the version leg is live. `crates/north-timers/src/timers/skill_triggers.rs:61` compares each
  `<tool>-version:` with `<tool> --version` (north#112, `7ba1919a1`), and nix-development has `cadence: 14`. The
  plan's other two legs were never built, and no box owned them:
  - Lock leg: `skill_triggers.rs` reads no `nixpkgs-lock:`, so nothing compares it with a `flake.lock`.
  - Watch leg: `RepositorySources::changed_since` (`skill_triggers.rs:124`) fires only for a skill's `sources:`
    entries. None of the three stamped files has a `sources:` line, so a cycle on this source flags nothing, and
    `skill_review` loads only `harness/skills`, so it never reads `cards.toml` or firn's skill.

  Both legs stay unbuilt; see Decisions. The plan's paragraph in nix-footguns.md now says what exists.

## Ranking

Ranked by expected effect on landing speed, quality and usage per closed box.

1. The due rule that launches these cycles: appreciable, measured (Experiments). With 51 of 58 counted commits
   being merges and 8 entries that change nothing north or firn runs, the cost of following the source is the
   dominant effect this cycle found.
2. GNOME 51: below the bar for now, kept as one check for firn's switch to 26.11. Five firn modules use GNOME
   packages (two keyring modules, `gnome-screenshot`, `xdg-desktop-portal-gnome` and `gnome-themes-extra`). None of
   it moves before the switch: firn's system nixpkgs follows nixos-26.05 (`flake.nix:4`, locked 2026-11-05), and the
   stamped files teach no GNOME option (`rg` count 0). Reasoned basis for the check: NixOS evaluation fails on a
   renamed or removed option but passes a daemon whose behaviour changed.
3. `xdg.directories`: dropped. It is opt-in, firn does not enable it (`rg` count 0), and firn sets
   `XDG_CACHE_HOME` only on two system services, which a session variable does not reach. Enabling it would not
   move watch-review's cache, which already falls back from `XDG_CACHE_HOME` to `~/.cache`
   (`crates/north-timers/src/timers/watch_review/system.rs:20`). North mentions these variables 131 times in 99
   files, not audited one by one, and nothing proposes enabling the module.
4. Falco, OpenLogi, phrog, LaSuite Docs, tdarr: dropped. Firn has no `falco`, `phrog` or `phosh`, `lasuite` or
   `tdarr` line and configures no HID++ tool; north uses none of them.

Measured over the whole 26.11 notes at `68060cbcdcd1` (244 entries since 26.05 branched): no entry names an option
under `nix.`, `nixos-rebuild`, flakes, `nix-daemon` or channels, and the stamped files name no NixOS option
(`nix.`, `services.`, `programs.`, `systemd.`, `environment.`, `boot.`, `users.`, `networking.`: 0 matches).
What the cards teach is the Nix command line (38 `nix develop`, `nix build`, `nix flake`, `nix fmt`, `nix run`,
`nix shell`, `nix eval` and `nix store` mentions), which the separately followed Nix release notes cover. This
source therefore serves firn's switch to each new NixOS release, not the cards.

## Experiments

Frozen input: `~/.cache/watch-review/nixos-release-notes.git` at `68060cbcdcd1`; one review day per day from
2026-09-10 to 2026-11-10, with the head taken as master's first-parent commit at 02:00 UTC that day; the source is
reviewed whenever the rule says it is due, and the next count starts from that day's head. Metric: cycles launched.
Keep rule, fixed when the replay script was written and before its output: keep when cycles fall and the content
(non-merge) commits reviewed over the window stay the same.

- Count content commits only (`git rev-list --count --no-merges`): 17 cycles fell to 11, and 127 content commits
  were reviewed either way. Verdict: keep. The other followed sources over the same window had these merges among
  their counted commits: pstack 0 of 86, ponytail 0 of 147, superpowers 0 of 6, t3code 0 of 6,103, nix-dev 0 of 1,
  nix-release-notes 0 of 0, mattpocock-skills 19 of 94, addyosmani-agent-skills 132 of 291 and impeccable 219 of
  933. The last three still pass 10 content commits within days, so only this source's cadence moves.
- A per-source monthly cadence (`cadence = "monthly"` in south `agents/watch.toml` drops the 10-commit trigger and
  leaves due-after-30-days-if-moved): 11 cycles fell to 2 (2026-10-10 with 50 content commits, 2026-11-09 with 77),
  and 127 content commits were reviewed either way. Verdict: keep. An entry now waits up to 30 days for review,
  which costs nothing before firn's switch, because a 26.11 entry acts on firn only at that switch.

Together the two changes take this source from 17 to 2 xhigh research cycles per 61 days.

## Decisions

- Landed with this record: watch-review counts `--no-merges` (`content_commits` in
  `crates/north-timers/src/timers/watch_review/system.rs`, test
  `a_merge_carrying_two_branches_changes_across_counts_as_no_new_commit`) and a source may set
  `cadence = "monthly"` (`crates/north-timers/src/timers/watch_review/mod.rs`, test
  `a_monthly_source_waits_out_its_month_however_many_commits_arrive`). South's `agents/watch.toml` sets the monthly
  cadence for this source once this lands.
- No card or skill is regrounded: none of the 8 entries touches what the stamped files teach.
- The trigger's lock and watch legs stay unbuilt. The watch leg has nothing to catch from this source, and wired as
  written it would flag the stamped files on every cycle, because `watch_reviewed_after` compares only
  `last_reviewed` with `grounded`. A lock move to a new release is already caught within nix-development's 14-day
  cadence, and the rest is the release check below.
- The first cycle after NixOS 26.11 ships reads the final 26.11 notes against firn before firn's switch, which has
  to land before 26.05 leaves support on 2026-12-31 ([nix-current-practice.md](../nix-current-practice.md)). From
  this range, the one entry to check is GNOME 51 against firn's keyring modules; the host-level check is held with
  the source's entry in south `agents/watch.toml`.
