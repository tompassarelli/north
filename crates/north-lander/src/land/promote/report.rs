use super::{NO_VERDICT, Outcome, Runtime, USAGE, link_hold, parse, promote, served_units, telemetry};
use crate::land::unit_start;
use std::fs;

pub fn run(arguments: &[String]) -> u8 {
    if arguments.iter().any(|argument| argument == "-h" || argument == "--help") {
        println!("{USAGE}");
        return 0;
    }
    let Some(options) = parse(arguments) else {
        eprintln!("{USAGE}");
        return 64;
    };
    let runtime = Runtime::from_env();
    if !options.dry_run {
        unit_start::page(&runtime.home, &served_units(&options.repos));
    }
    match promote(&options) {
        Ok(Outcome::Moved(result)) => {
            println!("north land promote: {result}");
            if !options.dry_run {
                link_hold::clear(&runtime.home);
            }
            0
        }
        Ok(Outcome::MovedUnjudged { candidate, reason }) => {
            println!("north land promote: no verdict: {reason}; the link stays on {candidate} and the next promotion replays it");
            if !options.dry_run {
                link_hold::unreached(&runtime.home, &candidate, &reason);
            }
            NO_VERDICT
        }
        Ok(Outcome::Rolled { candidate, held, reason }) => {
            if !options.dry_run {
                link_hold::release_rollback(&runtime.home);
            }
            let back = if options.dry_run { "would go back" } else { "is back" };
            println!("north land promote: refused: {reason}; the lander link {back} at {held} and {candidate} stays off the lander until current moves on");
            if !options.dry_run {
                link_hold::held(&runtime.home, telemetry::seconds(), &candidate, &reason);
            }
            1
        }
        Ok(Outcome::Kept { gone, reason }) => {
            if !options.dry_run {
                link_hold::release_rollback(&runtime.home);
            }
            println!("north land promote: refused: {reason}; the held runtime {gone} is no longer on disk, so the lander link stays on the runtime it runs now");
            1
        }
        Ok(Outcome::Settled(result)) => {
            println!("north land promote: {result}");
            if !options.dry_run {
                link_hold::clear(&runtime.home);
            }
            0
        }
        Err(error) => {
            println!("north land promote: refused: {error}");
            if !options.dry_run {
                let candidate = fs::read_link(runtime.current()).ok().and_then(|target| target.file_name().map(|name| name.to_string_lossy().into_owned())).unwrap_or_default();
                link_hold::held(&runtime.home, telemetry::seconds(), &candidate, &error);
            }
            1
        }
    }
}
