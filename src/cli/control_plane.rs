//! `north control-plane where|gate|move`: which host holds the lease that runs the landers, health and the other
//! control-plane units, the gate each unit starts through, and the break-glass move to another holder.
use std::process::Command;

use north_control_plane::{Lease, cas, is_host, moving, read, this_host};

const USAGE: &str = "usage: north control-plane where | gate UNIT [-- COMMAND...] | move --to HOST | move --finish | move --abort";

pub fn run(arguments: &[String]) -> u8 {
    let north = match std::env::current_exe() {
        Ok(north) => north,
        Err(error) => return fail(format!("cannot find the north binary: {error}")),
    };
    match arguments {
        [word] if word == "where" => where_(&north),
        [word, unit, rest @ ..] if word == "gate" => gate(&north, unit, rest),
        [word, flag, host] if word == "move" && flag == "--to" => move_to(&north, host),
        [word, flag] if word == "move" && flag == "--finish" => move_end(&north, true),
        [word, flag] if word == "move" && flag == "--abort" => move_end(&north, false),
        _ => {
            eprintln!("{USAGE}");
            2
        }
    }
}

fn where_(north: &std::path::Path) -> u8 {
    match read(north) {
        Ok(lease) => {
            println!("holder={} epoch={}", lease.holder, lease.epoch);
            0
        }
        Err(why) => fail(why),
    }
}

/// Runs `command` only on the holder. Every other host exits 0 at once; an unreadable lease runs nothing and fails.
fn gate(north: &std::path::Path, unit: &str, rest: &[String]) -> u8 {
    let lease = match read(north) {
        Ok(lease) => lease,
        Err(why) => return fail(format!("{unit}: {why}; not started")),
    };
    match decide(&lease, &this_host(), rest) {
        Gate::Skip => 0,
        Gate::Run(command) => run_command(command),
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Gate<'a> {
    Skip,
    Run(&'a [String]),
}

fn decide<'a>(lease: &Lease, host: &str, rest: &'a [String]) -> Gate<'a> {
    match rest {
        [dash, command @ ..] if dash == "--" && lease.holder == host && !command.is_empty() => Gate::Run(command),
        _ => Gate::Skip,
    }
}

fn run_command(command: &[String]) -> u8 {
    match Command::new(&command[0]).args(&command[1..]).status() {
        Ok(status) => u8::try_from(status.code().unwrap_or(1)).unwrap_or(1),
        Err(error) => fail(format!("cannot run {}: {error}", command[0])),
    }
}

/// The break-glass handover: the lease goes to `moving:OLD->NEW` at epoch + 1, then to NEW at epoch + 2. This writes
/// only the lease; stopping the old holder's units and copying its queue are later steps of the plan.
fn move_to(north: &std::path::Path, to: &str) -> u8 {
    if !is_host(to) {
        return fail(format!("{to} is no host name: letters, digits and dashes only"));
    }
    let from = match read(north) {
        Ok(lease) => lease,
        Err(why) => return fail(why),
    };
    if from.holder == to {
        return fail(format!("{to} already holds the lease at epoch {}", from.epoch));
    }
    if moving(&from.holder).is_some() {
        return fail(format!("a move is already under way: the lease reads {} at epoch {}; finish it with move --finish or undo it with move --abort", from.holder, from.epoch));
    }
    let moving = format!("moving:{}->{to}", from.holder);
    let parked = match cas(north, from.epoch, &moving) {
        Ok(lease) => lease,
        Err(why) => return fail(why),
    };
    match cas(north, parked.epoch, to) {
        Ok(lease) => {
            println!("holder={} epoch={}", lease.holder, lease.epoch);
            0
        }
        Err(why) => fail(why),
    }
}

/// Resolves a move that is under way: `finish` hands the lease to NEW, `abort` gives it back to OLD, one compare-and-swap either way.
fn move_end(north: &std::path::Path, finish: bool) -> u8 {
    let lease = match read(north) {
        Ok(lease) => lease,
        Err(why) => return fail(why),
    };
    let Some((from, to)) = moving(&lease.holder) else {
        return fail(format!("no move is under way: the lease reads {} at epoch {}", lease.holder, lease.epoch));
    };
    let holder = if finish { to } else { from };
    match cas(north, lease.epoch, holder) {
        Ok(lease) => {
            println!("holder={} epoch={}", lease.holder, lease.epoch);
            0
        }
        Err(why) => fail(why),
    }
}

fn fail(why: String) -> u8 {
    eprintln!("north control-plane: {why}");
    1
}
