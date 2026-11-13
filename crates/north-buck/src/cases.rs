//! `north buck compare-cases NAME...`: the cases Buck2 runs of each named crate at the checked-out commit beside those
//! nextest lists for its library, binaries and integration tests, so a crate is listed for Buck2's tests only once both run the same cases.
use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;

/// Reads the cases one side runs of a crate in a checkout, each as `BINARY-ID CASE`.
pub type CaseReader = fn(&Path, &str) -> Result<BTreeSet<String>, String>;

/// The two sides that must run the same cases of a crate before Buck2 tests it in Cargo's place.
pub struct Sides {
    pub buck2: CaseReader,
    pub nextest: CaseReader,
}

/// Prints each crate's count of cases on both sides and every case on one side only; `false` when a side could not be read, the sides differ or a crate has no case.
pub(crate) fn compare(root: &Path, names: &[String], sides: &Sides, out: &mut dyn Write) -> bool {
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
        _ = writeln!(out, "{name}: Buck2 ran {} case(s), nextest lists {}", buck2.len(), nextest.len());
        for case in buck2.difference(&nextest) {
            _ = writeln!(out, "  only Buck2 ran {case}");
        }
        for case in nextest.difference(&buck2) {
            _ = writeln!(out, "  only nextest lists {case}");
        }
        same &= buck2 == nextest && !buck2.is_empty();
    }
    same
}
