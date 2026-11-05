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
    let budget = field("Budget");
    let stated = |label: &str| budget.and_then(|line| dollars_after(line, label));
    let p90 = stated("p90");
    let p50 = stated("p50").or(p90);
    let minutes = field("ETA")
        .and_then(|eta| eta.split_whitespace().next()?.parse().ok())
        .unwrap_or(0.0);
    let source = p90.map_or(EstimateSource::None, |p90| EstimateSource::Budget(Usd(p90)));
    Request {
        provider: Provider::Claude,
        family,
        owner: Owner::Fleet,
        workstream,
        expected: Expected {
            p50: Usd(p50.unwrap_or_default()),
            p90: Usd(p90.unwrap_or_default()),
            minutes,
            source,
        },
        pin: None,
        host: Some(Host(host.to_owned())),
    }
}

fn dollars_after(text: &str, label: &str) -> Option<f64> {
    let rest = text.split_once(&format!("{label} $"))?.1;
    rest.split(|c: char| !(c.is_ascii_digit() || c == '.')).next()?.trim_end_matches('.').parse().ok()
}
