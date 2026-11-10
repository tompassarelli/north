# Private text judge: kinds of text kept off public repositories by construction

Tom asked on 2026-11-06 for one rule that keeps private kinds of text out of every public repository and in south (south#73). The deny-list already refuses private values; it cannot refuse a paragraph that names no listed value but still belongs in private.

## What runs

- `north check privacy judge --text FILE|-` reads a text, asks Haiku at low effort once per chunk with the kinds listed in south's `privacy/private-kinds.toml`, and prints one `kind<TAB>quoted span` line per finding. Exit 0 means clean, 1 means findings, 2 means the judge could not run. The kinds list lives only in south; north holds the method.
- A chunk Haiku flags is asked again of Sonnet at low effort three times at once, and a line is a finding only when at least two of the three answers quote a span that starts or ends in it, whatever kind each names; the finding reports the line itself and the kind most answers gave. Answers quoted the same line with different edges, so votes matched by span text split and the verdict changed between runs (north#701, 2026-11-07). An answer that reasons before ending on NONE is clean, and a finding under a kind id the list does not hold still counts, so neither ends the judge as unreadable. One Sonnet answer alone flipped between runs of the same text, so a reworded lane never converged (north#701, 2026-11-07). On long texts Haiku at low effort reports false findings that Sonnet at low effort does not (south#73 sweep rerun, 2026-11-07). Sonnet takes 6.5 to 13.3 s a text, so a clean text keeps Haiku's speed and only a flagged one pays for Sonnet.
- The call goes through the same one-shot path as the other model timers, with every variable that routes `claude` to token billing unset. With no subscription profile it falls back to the default login only when that login is a claude.ai OAuth login with no API-key variable and no apiKeyHelper; otherwise it refuses.
- Each chunk's answer from each model and run is cached by the hash of the prompt version, model, effort, run, kinds list and chunk under `~/.local/state/north/private-text-verdicts`, so a re-push or a repeated comment costs nothing and a changed kinds list asks again.
- A text with no prose never reaches the model: safe-push judges the outgoing commit messages, the tag annotation and the lines added to `.md`, `.txt`, `.rst` and `.adoc` files, so a code-only change judges its messages and nothing else. Each commit message, each file's added lines and the tag annotation are judged as chunks of their own, so rewording one file keeps the cached verdict of every other.

## Where it applies

- safe-push, after the deny-list scan, for a PUBLIC or UNKNOWN destination (`Denylist::verdict`). A finding refuses the push with `file it in south: <kind>`. A judge that cannot run queues the lane without an alone pass instead of refusing it, so the lander's own `--check-candidate` judge has another chance before the lane lands (north#1046). With neither a south checkout nor `NORTH_PRIVATE_KINDS`, it prints the same warning the deny-list prints and passes. A `SAFE_PUSH_SOUTH`, `SAFE_PUSH_DENYLIST` or `NORTH_PRIVATE_KINDS` that names a missing or empty source refuses instead, so only a host with no south at all, not an override, can skip both checks.
- The `public_text` hook, after the deny-list match, for every `gh` write to a repository that is not private. A finding, or a judge that cannot run, denies the command. An `issue edit` or `pr edit` judges only the lines its body adds to the current body, read with `gh issue view` or `gh pr view`, so ticking a box does not re-judge text already public; when the current body cannot be read, the whole body is judged. The hook waits 7 s, under the runtime's 10 s hook limit; a judge still running then, usually Sonnet's three confirming runs, is left to finish into the cache, and the command is refused with "judging, retry the same command in about 15 s", so the retry is answered from the cache.

A new project needs no setup: visibility comes from GitHub, as it does for the deny-list.

## Moving a refused text

The refusal names the kind. The private part goes to a south issue or `agents/decisions`; the public text keeps the generic method.

## Limits

The judge is a model: it can miss a rephrased detail and can flag an innocent one, and the quoted text it judges is untrusted data in its prompt. It is the second layer behind the literal deny-list, not a replacement.
