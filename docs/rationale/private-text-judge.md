# Private text judge: kinds of text kept off public repositories by construction

Tom asked on 2026-11-06 for one rule that keeps private kinds of text out of every public repository and in south (south#73). The deny-list already refuses private values; it cannot refuse a paragraph that names no listed value but still belongs in private.

## What runs

- `north privacy judge --text FILE|-` reads a text, asks the cheapest Claude model once per chunk with the kinds listed in south's `privacy/private-kinds.toml`, and prints one `kind<TAB>quoted span` line per finding. Exit 0 means clean, 1 means findings, 2 means the judge could not run. The kinds list lives only in south; north holds the method.
- The call goes through the same one-shot path as the other model timers, with every variable that routes `claude` to token billing unset. With no subscription profile it falls back to the default login only when that login is a claude.ai OAuth login with no API-key variable and no apiKeyHelper; otherwise it refuses.
- The verdict is cached by the hash of the prompt version, model, kinds list and text under `~/.local/state/north/private-text-verdicts`, so a re-push or a repeated comment costs nothing and a changed kinds list asks again.
- A text with no prose never reaches the model: safe-push judges the outgoing commit messages, the tag annotation and the lines added to `.md`, `.txt`, `.rst` and `.adoc` files, so a code-only change judges its messages and nothing else.

## Where it applies

- safe-push, after the deny-list scan, for a PUBLIC or UNKNOWN destination (`Denylist::verdict`). A finding refuses the push with `file it in south: <kind>`; so does a judge that cannot run. With neither a south checkout nor `NORTH_PRIVATE_KINDS`, it prints the same warning the deny-list prints and passes.
- The `public_text` hook, after the deny-list match, for every `gh` write to a repository that is not private. A finding, or a judge that cannot run, denies the command.

A new project needs no setup: visibility comes from GitHub, as it does for the deny-list.

## Moving a refused text

The refusal names the kind. The private part goes to a south issue or `agents/decisions`; the public text keeps the generic method.

## Limits

The judge is a model: it can miss a rephrased detail and can flag an innocent one, and the quoted text it judges is untrusted data in its prompt. It is the second layer behind the literal deny-list, not a replacement.
