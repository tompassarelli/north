use crate::error::NorthError;

const USAGE: &str = "Usage: north fleet [list [--json [--all]] | tree | spend [NAME | --self] | accounts | launch --provider claude|codex --account A [--cwd D] [--model M] [--effort low|medium|high|xhigh|max] [--name N] [--system-prompt FILE] [--prompt north|vanilla] [--foreground-cap SECS] [--idle-exit MINUTES] [--parent NAME|none] [--task REF [--force-dup REASON]] [--local REASON] [--pin REASON] [--tier-reason REASON] [--wait | --no-wait] [--test] PROMPT... \
     | send [--now] NAME TEXT... | relay HOST... | outbox take --for HOST | outbox drop ID... | outbox snapshot | row put | ask NAME QUESTION... [--timeout SECS] | interrupt NAME | stop NAME | forget NAME | stranded --json | rebooted [--revive] | revive NAME [--system-prompt FILE] [--effort LEVEL] [--local REASON] [--pin REASON] | show NAME [--follow | --json]]";

pub(super) fn usage() -> NorthError {
    NorthError::Usage(USAGE.into())
}
