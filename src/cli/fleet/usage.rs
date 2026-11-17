use crate::error::NorthError;

pub(in crate::cli) const USAGE: &str = "Usage: north fleet [list [--json [--all]] | tree | spend [NAME | --self] | adopt --name NAME | hold [--for LEAD] [--release] REPO#N | send [--now] [--from service:UNIT [--key CONDITION [--cleared]]] NAME TEXT... | row put | row exchange | row register | spool | ask NAME QUESTION... [--timeout SECS] | interrupt NAME | stop NAME | off [--force] | on | forget NAME [--force REASON] [--to SUCCESSOR] | stranded --json | interactive NAME | show NAME [--follow | --json] | hops NAME... | gate ... | orders list | digest | sweep-claims]";

pub(super) fn usage() -> NorthError {
    NorthError::Usage(USAGE.into())
}
