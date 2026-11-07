use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use super::quota;
use crate::platform::gh_broker_route::Place;
use crate::platform::gh_session::{self, Spend};
use crate::platform::github_app::{self, AGENT_APP, Host, Request};

const USAGE: &str = "usage: north ops github budget

Print what is left of each GitHub token's hourly limit (the user's own login and the Socrates Agent
App installation fleet sessions use), every fleet session's spend against its fair share
(the 400-unit fair share, see north gh-session) with its biggest spends by origin (hook, tick or command),
the north verb behind it and the gh endpoint, and the callers that cost the most GitHub
calls in the last hour by token (personal, app, none), personal first, from the broker's calls.log (calls GitHub answered with anything but 304,
apart from /rate_limit). A session reaching 50% and 80% of its fair share is told once.";

const TOP_CALLERS: usize = 10;
const TOP_CALLS: usize = 6;
const HOUR: u64 = 3600;

pub fn github(arguments: &[String]) -> u8 {
    match arguments.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["budget"] => budget(),
        ["-h" | "--help"] => {
            println!("{USAGE}");
            0
        }
        _ => {
            eprintln!("{USAGE}");
            2
        }
    }
}

fn budget() -> u8 {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_secs());
    let installation = Request::installation(AGENT_APP).ok_or_else(|| format!("unknown app {AGENT_APP}")).and_then(|request| github_app::mint(&request, &Host::from_env(), i64::try_from(now).unwrap_or(i64::MAX)));
    for (token, limits) in [("personal", quota::read(None)), ("socrates-agent app", installation.and_then(|token| quota::read(Some(&token))))] {
        match limits {
            Ok(limits) => limits.iter().for_each(|limit| println!("{token}\t{}", quota::line(limit))),
            Err(message) => println!("{token}\tunavailable: {message}"),
        }
    }
    let share = gh_session::fair_share();
    for (session, spent) in gh_session::session_spends(i64::try_from(now).unwrap_or(i64::MAX)) {
        let used = gh_session::used(&spent);
        println!("session {session}\t{used} of {share} units ({}%)", u64::from(used) * 100 / u64::from(share.max(1)));
        for (kind, caller, endpoint, units) in breakdown(&spent).into_iter().take(TOP_CALLS) {
            println!("  {kind}\t{caller}\t{endpoint}\t{units} units");
        }
    }
    let log = std::fs::read_to_string(Place::from_env().calls()).unwrap_or_default();
    for (caller, calls) in top_callers(&log, now).into_iter().take(TOP_CALLERS) {
        println!("caller {caller}\t{} calls in the last hour ({} personal, {} app, {} none)", calls.total(), calls.personal, calls.app, calls.none);
    }
    0
}

fn breakdown(spent: &[Spend]) -> Vec<(&'static str, String, String, u32)> {
    let mut units: HashMap<(&'static str, &str, &str), u32> = HashMap::new();
    for spend in spent {
        *units.entry((spend.kind.name(), &spend.caller, &spend.endpoint)).or_default() += spend.cost;
    }
    let mut rows: Vec<_> = units.into_iter().map(|((kind, caller, endpoint), units)| (kind, caller.to_owned(), endpoint.to_owned(), units)).collect();
    rows.sort_by(|left, right| right.3.cmp(&left.3).then_with(|| left.1.cmp(&right.1)).then_with(|| left.2.cmp(&right.2)));
    rows
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Calls {
    personal: u64,
    app: u64,
    none: u64,
}

impl Calls {
    fn total(&self) -> u64 {
        self.personal + self.app + self.none
    }
}

fn top_callers(log: &str, now: u64) -> Vec<(String, Calls)> {
    let mut spent: HashMap<&str, Calls> = HashMap::new();
    for line in log.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let (at, caller, source, status, target, token) = match fields.as_slice() {
            [at, caller, source, status, _method, target, token] => (at, caller, source, status, target, *token),
            [at, caller, source, status, _method, target] => (at, caller, source, status, target, "personal"),
            _ => continue,
        };
        let recent = at.parse::<u64>().is_ok_and(|at| now.saturating_sub(at) < HOUR);
        if recent && *source == "upstream" && *status != "304" && !target.starts_with("/rate_limit") {
            let calls = spent.entry(caller).or_default();
            match token {
                "app" => calls.app += 1,
                "none" => calls.none += 1,
                _ => calls.personal += 1,
            }
        }
    }
    let mut callers: Vec<(String, Calls)> = spent.into_iter().map(|(caller, calls)| (caller.to_owned(), calls)).collect();
    callers.sort_by(|left, right| right.1.personal.cmp(&left.1.personal).then_with(|| right.1.total().cmp(&left.1.total())).then_with(|| left.0.cmp(&right.0)));
    callers
}

#[cfg(test)]
mod tests {
    use super::{Calls, top_callers};

    #[test]
    fn top_callers_counts_only_the_last_hours_upstream_calls_that_cost_quota_by_token_and_ranks_personal_first() {
        let log = "9000\ta.service\tupstream\t200\tGET\t/repos/o/r/issues\tapp\n\
                   9100\ta.service\tupstream\t200\tGET\t/repos/o/r/pulls\tapp\n\
                   9100\tb.service\tupstream\t200\tGET\t/repos/o/r/pulls\tpersonal\n\
                   9100\tb.service\tupstream\t304\tGET\t/repos/o/r/pulls\tpersonal\n\
                   9100\tb.service\tfresh\t200\tGET\t/repos/o/r/pulls\tpersonal\n\
                   9100\tb.service\tupstream\t200\tGET\t/rate_limit\tpersonal\n\
                   100\tb.service\tupstream\t200\tGET\t/repos/o/r/issues\tpersonal\n";
        assert_eq!(top_callers(log, 9200), [("b.service".to_owned(), Calls { personal: 1, app: 0, none: 0 }), ("a.service".to_owned(), Calls { personal: 0, app: 2, none: 0 })]);
    }
}
