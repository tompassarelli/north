use std::collections::{BTreeMap, BTreeSet};
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use north_mutants::trees::RESULTS;

use super::flake::{self, Verdict};

const RUNS: usize = 1;

/// Builds the test executables of each tree with the host's build system and records, in each tree's `.mutant-results`, a `pass` or `fail` line for every named test its executables list. A tree whose tests do not compile records nothing, as a mutant that does not compile kills nothing.
pub fn run(trees: &[PathBuf], names: &BTreeSet<String>, out: &mut dyn Write) -> io::Result<()> {
    for tree in trees {
        match flake::executables(tree, out) {
            Ok(executables) => {
                let judged = flake::verdicts(tree, &executables, names, RUNS, out)?;
                record(tree, &judged)?;
            }
            Err(None) => writeln!(out, "mutant-runs: the tests of {} do not compile, so none of them runs", tree.display())?,
            Err(Some(why)) => return Err(io::Error::other(why)),
        }
    }
    Ok(())
}

fn record(tree: &Path, judged: &BTreeMap<String, Verdict>) -> io::Result<()> {
    let mut lines = String::new();
    for (name, verdict) in judged {
        match verdict {
            Verdict::Pass => lines.push_str(&format!("pass\t{name}\n")),
            Verdict::Flaky(_) => lines.push_str(&format!("fail\t{name}\n")),
            Verdict::Unlisted => {}
        }
    }
    OpenOptions::new().create(true).append(true).open(tree.join(RESULTS))?.write_all(lines.as_bytes())
}
