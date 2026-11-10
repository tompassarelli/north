# Usage resets: read the grant, never automate the click

Decided 2026-11-10 under north#1048, moved there from north#1065. The coordinator
will run on Fable high, and Fable has its own weekly meter. The question was whether
north may spend an account's offered usage reset when that meter reaches its floor
(98%, `north_pace::usage::decide::SUBSCRIPTION_FLOOR`). This note records what the
reset costs and whether the provider's terms allow automating it, before any code
clicks the account's page.

## What a reset is and what it costs

claude.ai's Settings > Usage page offers a "Full reset" and a "5-hour reset". A grant
says "reset for free" and carries an expiry; `usage-web-read` counts those grants per
kind and caches the counts and expiries in
`~/.local/state/agents/usage/claude-web.json`. The page shows no price for them, and
the reader records none: the resets the fleet can see are free. A reset the page does
not label free is treated as paid, and north performs no paid reset without a
per-reset spend cap Tom sets. No such cap is set, and no paid reset has been observed.

## Whether the terms allow automating it

No. Anthropic's Consumer Terms of Service (effective 2025-10-08), §3 "Use of our
Services", item 7:

> Except when you are accessing our Services via an Anthropic API Key or where we
> otherwise explicitly permit it, [you may not] access the Services through automated
> or non-human means, whether through a bot, script, or otherwise.

The reset exists only as a control on claude.ai's Settings > Usage page. Claude Code
has no command that resets a plan window: `/usage-credits` opens that page, and the
Claude Code CLI, Agent SDK and documented API expose no reset endpoint (the Claude
Code reference at docs.claude.com, read 2026-11-10). Browser automation on the
account's own profile would be automated access to the Services that the Terms do not
permit, so north does not build the browser path. `usage-web-read` stays a read; this
note governs performing a reset.

## What happens instead

- `north accounts` keeps showing each account's grants left and their expiry, so the
  fleet screen says when a reset is available.
- `north accounts reset [--dry-run]` checks the coordinator's account: it reads the
  cached grants and the account's Fable meter, then appends one JSON line per check to
  `~/.local/state/north/usage/resets.jsonl` with the account, the time, the meter
  before and after, the grants, and an outcome (`below-floor`, `no-grant` or
  `refused-terms`). It never clicks the page.
- When the coordinator's account reaches its Fable floor with a grant offered, the
  grant stays for Tom to use in his own signed-in browser.
- A future provider command or API, or Tom's explicit acceptance of the terms risk,
  can add a performing path. That path logs the same row with outcome `performed`, the
  path, and the meter after the reset, so the box's check is met by an observation
  outside the model.

## Decision

Do not automate claude.ai's reset control. Keep reading the grants, show them, and
revisit this note if Anthropic documents a reset API or explicitly permits the
automation.
