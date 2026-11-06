use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use super::quota;
use crate::platform::gh_broker_route::Place;
use crate::platform::gh_session;
use crate::platform::github_app::{self, AGENT_APP, Host, Request};

const USAGE: &str = "usage: north github budget

Print what is left of each GitHub token's hourly limit (the user's own login and the Socrates Agent
App installation fleet sessions use), every fleet session's spend against its hourly share
(NORTH_GH_HOURLY_BUDGET units, see north gh-session), and the callers that cost the most GitHub
calls in the last hour, from the broker's calls.log (calls GitHub answered with anything but 304,
apart from /rate_limit). A session reaching 50% and 80% of its share tells the proxy once.";

const TOP_CALLERS: usize = 10;
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
    let share = gh_session::hourly_budget();
    for (session, used) in gh_session::session_spends(i64::try_from(now).unwrap_or(i64::MAX)) {
        println!("session {session}\t{used} of {share} units ({}%)", u64::from(used) * 100 / u64::from(share.max(1)));
    }
    let log = std::fs::read_to_string(Place::from_env().calls()).unwrap_or_default();
    for (caller, calls) in top_callers(&log, now).into_iter().take(TOP_CALLERS) {
        println!("caller {caller}\t{calls} calls in the last hour");
    }
    0
}

fn top_callers(log: &str, now: u64) -> Vec<(String, u64)> {
    let mut spent: HashMap<&str, u64> = HashMap::new();
    for line in log.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let [at, caller, source, status, _method, target] = fields.as_slice() else {
            continue;
        };
        let recent = at.parse::<u64>().is_ok_and(|at| now.saturating_sub(at) < HOUR);
        if recent && *source == "upstream" && *status != "304" && !target.starts_with("/rate_limit") {
            *spent.entry(caller).or_default() += 1;
        }
    }
    let mut callers: Vec<(String, u64)> = spent.into_iter().map(|(caller, calls)| (caller.to_owned(), calls)).collect();
    callers.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    callers
}

#[cfg(test)]
mod tests {
    use super::top_callers;

    #[test]
    fn top_callers_counts_only_the_last_hours_upstream_calls_that_cost_quota_and_ranks_them() {
        let log = "9000\ta.service\tupstream\t200\tGET\t/repos/o/r/issues\n\
                   9100\ta.service\tupstream\t200\tGET\t/repos/o/r/pulls\n\
                   9100\tb.service\tupstream\t200\tGET\t/repos/o/r/pulls\n\
                   9100\tb.service\tupstream\t304\tGET\t/repos/o/r/pulls\n\
                   9100\tb.service\tfresh\t200\tGET\t/repos/o/r/pulls\n\
                   9100\tb.service\tupstream\t200\tGET\t/rate_limit\n\
                   100\tb.service\tupstream\t200\tGET\t/repos/o/r/issues\n";
        assert_eq!(top_callers(log, 9200), [("a.service".to_owned(), 2), ("b.service".to_owned(), 1)]);
    }
}
