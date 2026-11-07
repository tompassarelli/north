use super::{NO_VERDICT, Outcome, Runtime, USAGE, link_hold, parse, promote, swap, telemetry};
use std::fs;
use std::path::Path;

fn settle(link: &Path, target: &Path) -> u8 {
    if let Err(error) = swap(link, target) {
        println!("north land promote: refused: {}: {error}; the lander link is unchanged", link.display());
        return 1;
    }
    link_hold::clear(&Runtime::from_env().home);
    println!("north land promote: {} now points at {}; each lander whose inputs changed finishes its after-landing runs, exits and restarts on it", link.display(), target.display());
    0
}

pub fn run(arguments: &[String]) -> u8 {
    if arguments.iter().any(|argument| argument == "-h" || argument == "--help") {
        println!("{USAGE}");
        return 0;
    }
    let Some(options) = parse(arguments) else {
        eprintln!("{USAGE}");
        return 64;
    };
    match promote(&options) {
        Ok(Outcome::Swap { link, target }) => settle(&link, &target),
        Ok(Outcome::Unreached { candidate, reason }) => {
            println!("north land promote: no verdict: {reason}; a replay that reached no verdict says nothing about the candidate, so the link is neither held nor bisected, and the next promotion retries");
            if !options.dry_run {
                link_hold::unreached(&Runtime::from_env().home, &candidate, &reason);
            }
            NO_VERDICT
        }
        Ok(Outcome::Retry { candidate, reason }) => {
            println!("north land promote: flaky: {reason}; a flaky check says nothing about the candidate, so the link is not held, and the next promotion replays the candidate again");
            if !options.dry_run {
                link_hold::unreached(&Runtime::from_env().home, &candidate, &reason);
            }
            NO_VERDICT
        }
        Ok(Outcome::Settled(result)) => {
            println!("north land promote: {result}");
            if !options.dry_run {
                link_hold::clear(&Runtime::from_env().home);
            }
            0
        }
        Err(error) => {
            println!("north land promote: refused: {error}; the lander link is unchanged");
            if !options.dry_run {
                let runtime = Runtime::from_env();
                let candidate = fs::read_link(runtime.current()).ok().and_then(|target| target.file_name().map(|name| name.to_string_lossy().into_owned())).unwrap_or_default();
                link_hold::held(&runtime.home, telemetry::seconds(), &candidate, &error);
            }
            1
        }
    }
}
