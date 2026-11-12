use crate::error::NorthError;

pub(in crate::cli) const USAGE: &str = "Usage: north fleet [list [--json [--all]] | tree | spend [NAME | --self] | accounts [--api | --plan | --claude-ai | --reading ACCOUNT USD | --poll | --probe [LABEL]] | launch --provider claude|codex|opencode|pi [--account A] [--cwd D] [--model M] [--effort low|medium|high|xhigh|max] [--name N] [--system-prompt FILE] [--prompt north|vanilla] [--foreground-cap SECS] [--idle-exit MINUTES] [--parent NAME|none] [--role proxy|lead|worker] [--priority urgent|normal] [--task REF [--box TEXT]] [--route small-first|FAMILY[:EFFORT],FAMILY[:EFFORT]] [--local REASON] [--pin REASON] [--tier-reason REASON] [--wait | --no-wait] [--take-over] [--test] [--container-shell] PROMPT... \
     | adopt --name NAME | hold [--release] REPO#N | send [--now] [--from service:UNIT [--key CONDITION [--cleared]]] NAME TEXT... | row put | row exchange | row register | tick | ask NAME QUESTION... [--timeout SECS] | interrupt NAME | stop NAME | forget NAME [--force REASON] [--to SUCCESSOR] | stranded --json | rebooted [--revive] | revive NAME [--system-prompt FILE] [--effort LEVEL] [--local REASON] [--pin REASON] [--account LABEL] | relaunch NAME [--model M] [--effort LEVEL] | interactive NAME | show NAME [--follow | --json] | hops NAME... | reports --json | sweep-claims]";

pub(super) fn usage() -> NorthError {
    NorthError::Usage(USAGE.into())
}
