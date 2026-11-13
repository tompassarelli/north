//! `north buck compare-cases NAME...`: the cases Buck2 runs of each named crate at the checked-out commit beside those
//! nextest lists for its library, binaries and integration tests, so a crate is listed for Buck2's tests only once both
//! run the same cases. A host case of `build/host-cases.list` is Cargo's: nextest must list it and Buck2 must not run it.
use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;

use crate::host_class;

/// Reads the cases one side runs of a crate in a checkout, each as `BINARY-ID CASE`.
pub type CaseReader = fn(&Path, &str) -> Result<BTreeSet<String>, String>;

/// The two sides that must run the same cases of a crate before Buck2 tests it in Cargo's place.
pub struct Sides {
    pub buck2: CaseReader,
    pub nextest: CaseReader,
}

/// The host cases of the tree at `root` as `BINARY-ID CASE`, none where it keeps no list.
fn host_cases(root: &Path) -> Result<BTreeSet<String>, String> {
    Ok(host_class::reviewed(root, None)?.into_iter().map(|(binary, case)| format!("{binary} {case}")).collect())
}

/// Whether the case `BINARY-ID CASE` is of crate `name`: its unit tests' binary id is the crate's name, an integration test's `NAME::TEST`.
fn of(name: &str, case: &str) -> bool {
    case.split_once(' ').is_some_and(|(binary, _)| binary.split_once("::").map_or(binary, |(package, _)| package) == name)
}

/// Prints each crate's count of cases on both sides and every case on one side only; `false` when a side or the host cases could not be read, the sides differ, a listed host case is one nextest does not list, or a crate has no case.
pub(crate) fn compare(root: &Path, names: &[String], sides: &Sides, out: &mut dyn Write) -> bool {
    let host = match host_cases(root) {
        Ok(host) => host,
        Err(why) => {
            eprintln!("north buck compare-cases: {why}");
            return false;
        }
    };
    let mut same = true;
    for name in names {
        let (buck2, nextest) = match ((sides.buck2)(root, name), (sides.nextest)(root, name)) {
            (Ok(buck2), Ok(nextest)) => (buck2, nextest),
            (Err(why), _) | (_, Err(why)) => {
                eprintln!("north buck compare-cases: {name}: {why}");
                same = false;
                continue;
            }
        };
        let hosted: BTreeSet<String> = host.iter().filter(|case| of(name, case)).cloned().collect();
        let cargo: BTreeSet<String> = nextest.difference(&hosted).cloned().collect();
        _ = writeln!(out, "{name}: Buck2 ran {} case(s), nextest lists {}, {} of them host cases Cargo runs", buck2.len(), nextest.len(), nextest.intersection(&hosted).count());
        for case in hosted.difference(&nextest) {
            _ = writeln!(out, "  {} lists {case}, which nextest does not list", host_class::LIST);
        }
        for case in buck2.difference(&cargo) {
            _ = writeln!(out, "  only Buck2 ran {case}");
        }
        for case in cargo.difference(&buck2) {
            _ = writeln!(out, "  only nextest lists {case}");
        }
        same &= buck2 == cargo && hosted.is_subset(&nextest) && !buck2.is_empty();
    }
    same
}
