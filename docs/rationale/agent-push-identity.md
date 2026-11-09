# Agent push identity: agents push as the Socrates Agent app, Tom keeps his login

Tom approved on 2026-11-06 moving agent pushes off his admin login, so that GitHub itself refuses an agent's stray tag instead of safe-push alone (north#619).

## Before

Every git push from an agent session went through `gh auth git-credential`, which answers with Tom's own OAuth token. The two tag rulesets applied on 2026-11-05 to the 16 active repositories ("version tags" for `refs/tags/v*`, "non-version tags" for every other tag, both rule `creation`) let only the admin role bypass, and that token is the admin, so the rulesets bound every account except the agents.

## Decision

- **Identity.** Fleet sessions push as the Socrates Agent GitHub App (app id 5265857), installed on all of Tom's repositories with `contents: write` and `workflows: write`. The north lander App is installed on north alone, so it cannot carry pushes to the other repositories; it stays the lander's identity for north's main (`.safe-push` `landing.push-app = lander`).
- **Mechanism.** `north git-credential` is a git credential helper that answers `get` for `https://github.com` with the App's cached installation token (`north broker github-token`, limited to the repository the push names) and nothing for any other host. The Claude fleet runtime sets it per session through `GIT_CONFIG_COUNT`: an empty `credential.https://github.com.helper` drops the inherited `gh` helper, then the north helper follows. Where each host takes the token from, and the order of its fallbacks, is in south `agents/decisions/infra/agent-push-credential-detail-2026-11-07.md`. `north gh-session` takes its token from the same function (`github_app::agent_token`), so the two helpers cannot drift (north#668). The session also sets `credential.https://github.com.useHttpPath`, so git sends that path. No config file changes, so Tom's own git and `gh` login are unchanged.
- **Releases.** `north land release` pushes its `v*` tag as the agent identity; the repository rules that allow it are in south `agents/decisions/infra/agent-push-credential-detail-2026-11-07.md`. Non-version tags have no App bypass: GitHub refuses them from an agent session.

## Alternatives not taken

- Give the north lander App the `v*` bypass and have `north land release` mint its token: not taken, because safe-push already refuses `v*` tags outside `north land release` and the tripwire refuses a raw `git push`, so the extra GitHub-side guard buys little; the credential cost is in south `agents/decisions/infra/agent-push-credential-detail-2026-11-07.md`.

## Observed

On 2026-11-06 a `north safe-push --tag probe-2026-11-06` from a session with the fleet's git environment was refused by GitHub (`GH013: Repository rule violations ... Cannot create ref due to creations being restricted`), and the lane that recorded this note was pushed with the same identity and landed by the lander.

## API calls and the per-session budget (north#636)

On 2026-11-06 at 4:05 AM one worker's CI-history scraping spent Tom's 5000-call hourly GitHub limit, which stopped dispatch, issue edits, the CI watch and the sheriff until 4:31 AM. Sessions called the API with Tom's token, so one session could starve every tool that shares it.

- **Identity.** `bin/gh` is a PATH shim in `~/.local/share/north/bin`. In a fleet session (`NORTH_FLEET_SESSION` set) it runs the real `gh` through `north gh-session`, which sets `GH_TOKEN` to a fresh installation token of the Socrates Agent App and removes `GITHUB_TOKEN` and the enterprise variants. Outside a session the shim runs the real `gh` unchanged. A session that cannot get the App token is refused; it never falls back to Tom's login. The token is minted per call from the cached installation token, so it never expires under a long session, unlike a `GH_TOKEN` fixed at launch.
- **Budget.** A session spends GitHub API units counted per rolling hour in `$XDG_RUNTIME_DIR/north-gh-budget/SESSION`: a call costs 1 unit, `--paginate` 20, and `--limit N` one per 100 rows; help, version and config cost none. A call is refused only when the shared installation token is short: while its remaining hourly quota (`gh api rate_limit`, read at most once a minute) is at least 1000 units above the call's cost every session may spend; under that reserve the sessions whose hour's spend is at or above the average of the live sessions are refused first (exit 75, naming the token's remaining units and the session's spend), so one scraper cannot starve the rest while a session is never held back from a token with most of its hour left (north#726; a fixed 400-unit cap per session had refused sessions while the token still had most of its hour left). 400 units remains the fair share the 50% and 80% notices and a hook's tenth are measured against. The units estimate calls from the command line because `gh` does not report how many requests it made.
- **Consequence for writes.** Issue, comment and pull request writes from a session now run as the App, so they depend on the App's installed permissions.
