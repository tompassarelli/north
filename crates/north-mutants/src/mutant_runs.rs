use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use north_landing_gates::landing::flake::{self, Reported};
use north_landing_gates::landing::trees::{RESULTS, UNLISTED};


const RUNS: usize = 1;
pub const SHARED: &str = "mutant-src";

/// Builds the test executables of each tree with the host's build system and records, in each tree's `.mutant-results`, a `pass` or `fail` line for every named test whose own line its executables print; a test without that line records nothing. A tree whose tests do not compile records nothing, as a mutant that does not compile kills nothing. Each tree is mirrored into `shared` before its build, so every tree builds and runs in one directory whose target is warm: rsync's checksum comparison gives a new modification time to exactly the files that differ from the last build, which is what cargo's staleness check reads.
pub fn run(trees: &[PathBuf], names: &BTreeSet<String>, shared: &Path, out: &mut dyn Write) -> io::Result<()> {
    fs::create_dir_all(shared)?;
    for tree in trees {
        mirror(tree, shared)?;
        match flake::executables(shared, out) {
            Ok(executables) => {
                let reported = flake::reported(shared, &executables, names, RUNS);
                for name in names.iter().filter(|name| !reported.contains_key(*name)) {
                    writeln!(out, "{UNLISTED}{name}")?;
                }
                record(tree, &reported)?;
            }
            Err(None) => writeln!(out, "mutant-runs: the tests of {} do not compile, so none of them runs", tree.display())?,
            Err(Some(why)) => return Err(io::Error::other(why)),
        }
    }
    Ok(())
}

fn mirror(tree: &Path, shared: &Path) -> io::Result<()> {
    let status = Command::new("rsync")
        .args(["-rlpc", "--delete", "--exclude=.mutant-results", "--exclude=/target", "--exclude=/.git"])
        .arg(format!("{}/", tree.display()))
        .arg(format!("{}/", shared.display()))
        .stdin(Stdio::null())
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("rsync of {} into {} exited {status}", tree.display(), shared.display())))
    }
}

fn record(tree: &Path, reported: &BTreeMap<String, Reported>) -> io::Result<()> {
    let mut lines = String::new();
    for (name, verdict) in reported {
        match verdict {
            Reported::Passed => lines.push_str(&format!("pass\t{name}\n")),
            Reported::Failed => lines.push_str(&format!("fail\t{name}\n")),
            Reported::Silent => {}
        }
    }
    OpenOptions::new().create(true).append(true).open(tree.join(RESULTS))?.write_all(lines.as_bytes())
}
