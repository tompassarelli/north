# GitHub Actions

Read this to write or change workflows, dispatch or wait on runs, or run many agents against GitHub. GitHub's published limits: [Actions usage limits](https://docs.github.com/en/actions/reference/limits); read [sources](actions-sources.md) only when revising a limit or resolving its source. Command detail is in `gh api --help`, `gh workflow run --help` and `gh run --help`.

## Shared budgets

- Share the account's 40 concurrent jobs (GitHub Pro; at most 5 macOS) across repositories, because every repository and agent draws from one pool.
- Share Tom's 5,000 REST calls per hour across tokens, apps and agents, and budget workflow `GITHUB_TOKEN` separately at 1,000 per hour per repository, because exhausting either stalls every agent.
- Respect secondary limits of 100 concurrent requests, 900 points per minute and 80 content-creating requests per minute, because they refuse independently of the hourly budget.

## Workflows

- Limit a workflow to about 8 concurrent jobs with `strategy.max-parallel`, so CI's first job can start within one minute.
- Use fewer, larger shards, because each shard pays checkout, setup and queue cost.
- Treat the legal 256-job matrix as a ceiling, not a target, because it alone exceeds the account pool.
- Route heavy shard jobs with `runs-on: ${{ vars.FARM_RUNNER || 'ubuntu-latest' }}` (smashcraft and wisp: the `farm-big` self-hosted box, never for pull requests), and delete the variable when the box is gone, because those jobs otherwise queue instead of falling back to hosted runners.
- Set CI concurrency to `${{ github.workflow }}-${{ github.ref }}` with `cancel-in-progress: true`, and keep fixed non-cancelling groups for landings only, because superseded CI wastes slots while a cancelled landing loses work.
- Exclude farm/tests and account for replacement of older pending runs, because a pending run in a group is replaced by the next.
- Dispatch Balance, Playtest and soak runs only on a commit already on main, in a per-workflow `cancel-in-progress` group, and cancel them when that commit is reverted, because results on other commits answer nothing.
- Cap a batched landing queue at 2 lanes per batch until it tests each lane on its own first, because batches of 3 or more went 0 of 15 green.

## Dispatching and cleanup

- Count queued and running jobs before large dispatches with `gh api repos/OWNER/REPO/actions/runs/ID/jobs`, counting a missing `runner_name` as waiting, and wait or shrink dispatches near the account limit, because queued jobs still hold the pool.
- Join an existing queued or running same-workflow run on the commit, and never dispatch or push a scratch branch for a duplicate run, because duplicates double the cost.
- Cancel superseded pushes, abandoned experiments and unnecessary fields with `gh run cancel ID`, and never cancel a landing-queue run, because landing runs carry other agents' work.
- Register scratch-branch deletion at creation through a trap, finally or workflow cleanup, because cleanup added later is skipped on failure.

## Waiting and rate limits

- Wait through the repository's own waiting tool and implement a waiter only when none exists, because waiters multiply API calls.
- Poll after 10 seconds, multiply unchanged intervals by 1.5 up to 60 seconds, and reset to 10 seconds on job starts or finishes, using one or two API calls per poll, because twenty waiters at the 60-second cap already spend about 2,400 calls per hour.
- Batch job reads, make requests sequentially and prefer conditional requests, because 304 responses do not count against the budget.
- Treat rate-limit HTTP 403 and 429 as real and inspect the refused response headers rather than trusting `gh api rate_limit`, because the headers name the limit that refused.
- Wait for `x-ratelimit-reset` when remaining is zero, or honor `retry-after` for secondary limits, and never retry immediately while refused, because early retries extend the refusal.
- Retry every tooling `gh` rate-limit refusal with header delays or 30-second exponential backoff capped at five minutes, with jitter, for about 65 minutes across the hourly reset, printing waits to stderr, because the budget recovers only at reset.
- Stop your own polling when refusals start, and never add an agent to check them, because every extra caller spends the same budget.
