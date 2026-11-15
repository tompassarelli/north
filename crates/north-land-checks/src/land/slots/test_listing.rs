//! `north land list-tests [PACKAGE...]`: builds and lists the tests of a land check's selection on the host that runs
//! the listing, the workspace when no package is named. A host whose `[hosts.NAME] buck2_listing` is true lists them with
//! Buck2, which builds the test binaries on the workers from the shared store; any other host on the side it builds
//! with, compiling them first where that side lists only what a build left. It prints the listing as one [`LISTED`] line,
//! the same whichever side listed, so the hand-in that formed the request parses it without knowing the side (north#1508).

use std::path::Path;
use std::process::Stdio;

use north_build_system::{BuildSystem, Resolution, Selection, Shell, Step, TestBinary, Tests};
use serde::{Deserialize, Serialize};

use super::unit_reuse::quote;

const USAGE: &str = "north land list-tests available | north land list-tests -- [PACKAGE...]";
const FAILED: u8 = 1;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Listed {
    listed: Vec<TestBinary>,
}

/// `listing` handed to the executing host's `north`, which lists `packages` there, the workspace when empty, or run as it is where that north has no such verb or the host lists with the side the hand-in formed the line for.
pub fn wrapped(north: &str, packages: &[String], listing: &str) -> String {
    let named = packages.iter().map(|package| quote(package)).collect::<Vec<_>>().join(" ");
    format!("n={north}; if \"$n\" land list-tests available >/dev/null 2>&1; then exec \"$n\" land list-tests -- {named}; fi\n{listing}")
}

/// `compile`, the build of the test binaries before a listing, skipped on a host where `north land list-tests` lists, since that listing builds what it lists.
pub fn skipped(north: &str, compile: &str) -> String {
    format!("n={north}; if \"$n\" land list-tests available >/dev/null 2>&1; then exit 0; fi\n{compile}")
}

/// The test binaries `printed` holds when `north land list-tests` printed it.
pub fn parsed(printed: &str) -> Option<Vec<TestBinary>> {
    printed.lines().find_map(|line| serde_json::from_str::<Listed>(line).ok()).map(|listed| listed.listed)
}

fn listed(side: &dyn BuildSystem, root: &Path, selection: Selection) -> Result<Vec<TestBinary>, String> {
    let request = |step| Tests { root, step, selection: selection.clone(), resolution: Resolution::PINNED, shell: Shell::LandShell, output: Some(Path::new("target")), uncached: false, coverage: None };
    if !side.lists_by_building() {
        let status = side.tests(&request(Step::Compile)).command().stdin(Stdio::null()).stdout(Stdio::null()).status().map_err(|error| error.to_string())?;
        if !status.success() {
            return Err(format!("the build of the test binaries exited {status}"));
        }
    }
    let output = side.tests(&request(Step::List)).command().stdin(Stdio::null()).stderr(Stdio::inherit()).output().map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!("the listing exited {}", output.status));
    }
    side.listing(&String::from_utf8_lossy(&output.stdout))
}

fn list(packages: &[String]) -> u8 {
    let Ok(root) = std::env::current_dir() else { return FAILED };
    let side = north_build_system::listing_side(&root).unwrap_or_else(|| north_build_system::for_tree(&root));
    let selection = if packages.is_empty() { Selection::Workspace } else { Selection::Packages(packages.to_vec()) };
    match listed(side.as_ref(), &root, selection).and_then(|listed| serde_json::to_string(&Listed { listed }).map_err(|error| error.to_string())) {
        Ok(line) => {
            println!("{line}");
            0
        }
        Err(why) => {
            eprintln!("north land list-tests: {why}");
            FAILED
        }
    }
}

pub fn run(arguments: &[String]) -> u8 {
    match arguments {
        [verb] if verb == "available" => u8::from(std::env::current_dir().ok().and_then(|root| north_build_system::listing_side(&root)).is_none()),
        [separator, packages @ ..] if separator == "--" => list(packages),
        _ => {
            eprintln!("usage: {USAGE}");
            64
        }
    }
}
