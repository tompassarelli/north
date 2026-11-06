use std::path::Path;

use north_base::foundation::step::Step;
use north_landing::land::scans::outgoing::denylist::Denylist;
use north_landing::land::scans::outgoing::{email, instruction_limits, language, native_ref, secrets};

pub struct TagScan<'a> {
    pub directory: &'a Path,
    pub root: &'a Path,
    pub origin_url: &'a str,
    pub tag: &'a str,
    pub annotation: &'a [u8],
    pub revisions: &'a [String],
    pub base: &'a str,
    pub target: &'a str,
    pub outgoing: bool,
}

pub fn scan(scan: &TagScan) -> Step {
    language::language_gate(scan.root, scan.base, scan.target)?;
    instruction_limits(scan.root, scan.target)?;
    let annotation = String::from_utf8_lossy(scan.annotation);
    native_ref::native_ref_scan(scan.directory, &[scan.tag, annotation.trim_end_matches('\n')], scan.revisions)?;
    if scan.outgoing {
        secrets::history_secret_scan(scan.directory, scan.revisions)?;
        email::commit_email_scan(scan.directory, scan.revisions)?;
    }
    Denylist::from_env().scan(scan.root, scan.origin_url, scan.base, scan.target, scan.tag)?;
    secrets::annotation_secret_scan(scan.directory, scan.annotation)
}
