//! `north land graph-build`: builds the whole Buck2 graph of the tree for a lane that changes a file defining a
//! target (north#1115), on the workers, and judges it by Buck2's exit status alone: 0 passes, 3 (the tree's own
//! targets or code failed) fails, any other Buck2 status says nothing of the tree. A host that holds no store generation, or a tree whose Buck2 configuration is not the base's, gets no graph build and passes, as the lint gate leaves its line as declared. The tests of
//! such a lane stay on Cargo.

use north_base::foundation::exit::NO_VERDICT;

const BASE_ENV: &str = "SAFE_PUSH_LAND_BASE";
const TREE_ERROR: i32 = 3;
const FAILED: u8 = 1;

fn verdict(status: Option<i32>) -> u8 {
    match status {
        Some(0) => 0,
        Some(TREE_ERROR) => FAILED,
        _ => NO_VERDICT,
    }
}

pub fn run(arguments: &[String]) -> u8 {
    if !arguments.is_empty() {
        eprintln!("usage: north land graph-build");
        return 64;
    }
    let base = std::env::var(BASE_ENV).ok().filter(|base| !base.is_empty());
    let Ok(root) = std::env::current_dir() else { return NO_VERDICT };
    let build = match north_build_system::graph_build(&root, base.as_deref()) {
        Ok(build) => build,
        Err(why) => {
            eprintln!("north land graph-build: {why}, so this lane gets no graph build here");
            return 0;
        }
    };
    eprintln!("north land graph-build: {}", build.words.join(" "));
    let status = build.command().status().ok().and_then(|status| status.code());
    let verdict = verdict(status);
    if verdict == NO_VERDICT {
        eprintln!("north land graph-build: buck2 exited {}, which says nothing of the tree, so the check has no verdict", status.map_or_else(|| "on a signal".to_owned(), |code| code.to_string()));
    }
    verdict
}
