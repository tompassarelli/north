use super::game_reserve;
use super::mode::{Limits, State, enforced_split, hold, now, read, relieve, release, state_dir, status_line, write};

const USAGE: &str = "usage: north capacity mode status | hold [--minutes N] [PHRASE] | release | relieve PERCENT [--minutes N]";

fn relieve_now(state: &State, arguments: &[&str]) -> Option<State> {
    let (percent, minutes) = match arguments {
        [percent] => (percent.parse::<u32>().ok()?, Limits::bundled().lease_minutes),
        [percent, "--minutes", minutes] => (percent.parse::<u32>().ok()?, minutes.parse::<u64>().ok()?),
        _ => return None,
    };
    let (mut next, lower) = relieve(state, percent.max(100), minutes, now());
    if let Some(quota) = lower {
        match game_reserve::set_agent_quota(&game_reserve::Settings::from_env(), quota) {
            Ok(()) => next.agent_quota_percent = Some(quota),
            Err(error) => eprintln!("north capacity mode: agent.slice CPUQuota {quota}%: {error}"),
        }
    }
    Some(next)
}

pub fn run(arguments: &[String]) -> u8 {
    let limits = Limits::bundled();
    let dir = state_dir();
    let state = read(&dir);
    let changed = match arguments.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["status"] => {
            println!("{}", status_line(&state, &limits, enforced_split().as_ref()));
            return 0;
        }
        ["hold"] => hold(&state, now(), "north capacity mode hold", limits.lease_minutes, &limits),
        ["hold", phrase] => hold(&state, now(), phrase, limits.lease_minutes, &limits),
        ["hold", "--minutes", minutes, rest @ ..] if rest.len() <= 1 => {
            let Ok(minutes) = minutes.parse::<u64>() else {
                eprintln!("{USAGE}");
                return 2;
            };
            hold(&state, now(), rest.first().copied().unwrap_or("north capacity mode hold"), minutes, &limits)
        }
        ["release"] => release(&state, now()),
        ["relieve", rest @ ..] => {
            let Some(relieved) = relieve_now(&state, rest) else {
                eprintln!("{USAGE}");
                return 2;
            };
            relieved
        }
        _ => {
            eprintln!("{USAGE}");
            return 2;
        }
    };
    match write(&dir, &changed) {
        Ok(()) => {
            println!("{}", status_line(&changed, &limits, enforced_split().as_ref()));
            0
        }
        Err(error) => {
            eprintln!("north capacity mode: {error}");
            1
        }
    }
}
