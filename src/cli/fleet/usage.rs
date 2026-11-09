use crate::error::NorthError;

pub(in crate::cli) const USAGE: &str = "Usage: north fleet [list [--json [--all]] | tree | spend [NAME | --self] | drain on [--until TIME] | drain off | accounts [--api | --plan | --claude-ai] | launch --provider claude|codex [--account A] [--cwd D] [--model M] [--effort low|medium|high|xhigh|max] [--name N] [--system-prompt FILE] [--prompt north|vanilla] [--foreground-cap SECS] [--idle-exit MINUTES] [--parent NAME|none] [--role proxy|lead|worker] [--task REF [--box TEXT]] [--local REASON] [--pin REASON] [--tier-reason REASON] [--wait | --no-wait] [--take-over] [--test] PROMPT... \
     | adopt --name NAME | hold REPO#N | send [--now] NAME TEXT... | row put | row exchange | row register | tick | ask NAME QUESTION... [--timeout SECS] | interrupt NAME | stop NAME | forget NAME [--force REASON] [--to SUCCESSOR] | stranded --json | rebooted [--revive] | revive NAME [--system-prompt FILE] [--effort LEVEL] [--local REASON] [--pin REASON] [--account LABEL] | relaunch NAME [--model M] [--effort LEVEL] | interactive NAME | show NAME [--follow | --json] | reports --json | sweep-claims]";

pub(super) fn usage() -> NorthError {
    NorthError::Usage(USAGE.into())
}
