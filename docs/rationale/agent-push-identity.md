# Agent push identity: agents push as the Socrates Agent app, Tom keeps his login

Tom approved on 2026-11-06 moving agent pushes off his admin login, so that GitHub itself refuses an agent's stray tag instead of safe-push alone (north#619).

## Before

Every git push from an agent session went through `gh auth git-credential`, which answers with Tom's own OAuth token. The two tag rulesets applied on 2026-11-05 to the 16 active repositories ("version tags" for `refs/tags/v*`, "non-version tags" for every other tag, both rule `creation`) let only the admin role bypass, and that token is the admin, so the rulesets bound every account except the agents.

## Decision

- **Identity.** Fleet sessions push as the Socrates Agent GitHub App (app id 5265857), installed on all of Tom's repositories with `contents: write` and `workflows: write`. The north lander App is installed on north alone, so it cannot carry pushes to the other repositories; it stays the lander's identity for north's main (`.safe-push` `landing.push-app = lander`).
- **Mechanism.** `north git-credential` is a git credential helper that answers `get` for `https://github.com` with the App's cached installation token (`north gh-app-token`, key in south's sops file) and nothing for any other host. The Claude fleet runtime sets it per session through `GIT_CONFIG_COUNT`: an empty `credential.https://github.com.helper` drops the inherited `gh` helper, then the north helper follows. No config file changes, so Tom's own git and `gh` login are unchanged, and a session's `gh` API calls still run as Tom.
- **Releases.** The "version tags" ruleset on each of the 16 repositories lists the Socrates Agent App as a bypass actor beside the admin role (a user-owned repository accepts an `Integration` bypass actor; applied 2026-11-06), so `north release` pushes its `v*` tag as the agent identity. Non-version tags have no App bypass: GitHub refuses them from an agent session.

## Alternatives not taken

- Give the north lander App the `v*` bypass and have `north release` mint its token: GitHub would then refuse an agent's `v*` tag too, but the lander App would need installing on 15 more repositories and its key would leave the lander's credential boundary. Today safe-push refuses `v*` tags outside `north release` and the tripwire refuses a raw `git push`, so the extra GitHub-side guard buys little.
- Point `GH_TOKEN` at the App: issue, comment and pull request writes would become the bot's and depend on its permissions; out of scope for a tag guard.
