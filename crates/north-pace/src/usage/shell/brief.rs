use super::super::decide::world::{
    EstimateSource, Expected, Host, ModelFamily, Owner, Provider, Request, Usd, Workstream,
};
use super::launches::family;

pub fn request(brief: &str, host: &str) -> Request {
    let field = |name: &str| {
        brief
            .lines()
            .find_map(|line| line.strip_prefix(name)?.strip_prefix(':'))
            .map(str::trim)
    };
    let family = field("Tier").and_then(family).unwrap_or(ModelFamily::Opus);
    let workstream = field("Item")
        .and_then(|item| item.split_whitespace().next())
        .map(|item| Workstream(item.to_owned()));
    let amounts = field("Budget").map(dollars).unwrap_or_default();
    let p90 = amounts.iter().copied().fold(0.0, f64::max);
    let p50 = amounts.iter().copied().fold(p90, f64::min);
    let minutes = field("ETA")
        .and_then(|eta| eta.split_whitespace().next()?.parse().ok())
        .unwrap_or(0.0);
    let source = if amounts.is_empty() {
        EstimateSource::None
    } else {
        EstimateSource::Budget(Usd(p90))
    };
    Request {
        provider: Provider::Claude,
        family,
        owner: Owner::Fleet,
        workstream,
        expected: Expected {
            p50: Usd(p50),
            p90: Usd(p90),
            minutes,
            source,
        },
        pin: None,
        host: Some(Host(host.to_owned())),
    }
}

fn dollars(text: &str) -> Vec<f64> {
    text.split('$')
        .skip(1)
        .filter_map(|rest| {
            rest.split(|c: char| !(c.is_ascii_digit() || c == '.'))
                .next()?
                .trim_end_matches('.')
                .parse()
                .ok()
        })
        .collect()
}
