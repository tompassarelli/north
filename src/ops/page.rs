use super::expect::{Outcome, State};
use super::snapshot::Snapshot;

const STYLE: &str = "body{font:15px/1.45 system-ui,sans-serif;margin:24px;max-width:1100px;background:#fbfbf9;color:#1f2328}
h1{font-size:20px}h2{font-size:16px;margin-top:28px;border-bottom:1px solid #ddd}
li{margin:3px 0}.bad{color:#b42318;font-weight:600}.ok{color:#1a7f37}.dim{color:#6e7781}
.meter{display:inline-block;width:160px;height:9px;background:#e6e6e3;border-radius:5px;vertical-align:middle;margin:0 8px}
.meter i{display:block;height:9px;border-radius:5px;background:#2f6feb}.meter i.hot{background:#b42318}
@media(prefers-color-scheme:dark){body{background:#16181c;color:#e6e6e3}h2{border-color:#333}.meter{background:#30343a}}";

pub(crate) fn render(snapshot: &Snapshot) -> String {
    let sections = [
        section("Expectations", expectations(&snapshot.expectations)),
        section("Accounts", accounts(snapshot)),
        section("Where work ran", placement(snapshot)),
        section("Landing queues", landing(snapshot)),
        section("Bets", bets(snapshot)),
    ];
    let breached = snapshot.breached.len();
    let verdict = match breached {
        0 => "<p class=ok>Every expectation holds.</p>".to_owned(),
        n => format!("<p class=bad>{n} expectation{} breached: {}</p>", if n == 1 { "" } else { "s" }, escape(&snapshot.breached.join(", "))),
    };
    let gaps = snapshot.gaps.iter().map(|gap| format!("<li class=dim>{}</li>", escape(gap))).collect::<String>();
    format!(
        "<!doctype html><meta charset=utf-8><meta http-equiv=refresh content=300><title>north ops</title><style>{STYLE}</style>\
         <h1>north ops <span class=dim>{}</span></h1>{verdict}{}<ul>{gaps}</ul>",
        escape(&snapshot.at),
        sections.concat()
    )
}

fn section(title: &str, items: Vec<String>) -> String {
    format!("<h2>{title}</h2><ul>{}</ul>", items.concat())
}

fn expectations(outcomes: &[Outcome]) -> Vec<String> {
    let mut sorted: Vec<&Outcome> = outcomes.iter().collect();
    sorted.sort_by_key(|outcome| outcome.state != State::Breached);
    sorted
        .into_iter()
        .map(|outcome| {
            let class = match outcome.state {
                State::Breached => "bad",
                State::Holds => "ok",
                _ => "dim",
            };
            let value = outcome.value.map_or("unknown".to_owned(), |value| format!("{value:.2}"));
            format!(
                "<li><span class={class}>{}</span> {}: {} <span class=dim>({} = {value}, expected {} {} over {}; owner {}; fix: {})</span></li>",
                outcome.state.label(),
                escape(&outcome.id),
                escape(&outcome.behavior),
                escape(&outcome.metric),
                escape(outcome.op.symbol()),
                outcome.threshold,
                escape(&outcome.window),
                escape(&outcome.owner),
                escape(&outcome.fix)
            )
        })
        .collect()
}

fn meter(percent: Option<f64>) -> String {
    let percent = percent.unwrap_or(0.0).clamp(0.0, 100.0);
    format!("<span class=meter><i class={} style=width:{percent:.0}%></i></span>", if percent >= 80.0 { "hot" } else { "" })
}

fn accounts(snapshot: &Snapshot) -> Vec<String> {
    snapshot
        .accounts
        .iter()
        .map(|account| {
            let exhausts = account.hours_to_exhaustion.map_or("lasts to reset".to_owned(), |hours| format!("exhausts {} in {hours:.1} h", account.exhausts.as_deref().unwrap_or("")));
            let class = if account.hours_to_exhaustion.is_some_and(|hours| hours < 1.0) { "bad" } else { "" };
            format!(
                "<li>{} 5h{}{}% week{}{}% <span class={class}>{exhausts}</span> <span class=dim>{} live, {} working</span></li>",
                escape(&account.account),
                meter(account.five_hour_pct),
                account.five_hour_pct.map_or("?".into(), |pct| format!("{pct:.0}")),
                meter(account.week_pct),
                account.week_pct.map_or("?".into(), |pct| format!("{pct:.0}")),
                account.sessions_live,
                account.sessions_working
            )
        })
        .collect()
}

fn placement(snapshot: &Snapshot) -> Vec<String> {
    let total: f64 = snapshot.placement.iter().filter_map(|place| place.cpu_hours_24h).sum();
    snapshot
        .placement
        .iter()
        .map(|place| {
            let share = place.cpu_hours_24h.filter(|_| total > 0.0).map(|hours| hours / total * 100.0);
            format!(
                "<li>{}{}{} sessions, {} CPU-h in 24 h <span class=dim>({})</span></li>",
                place.label,
                meter(share),
                place.sessions_24h.map_or("?".into(), |n| n.to_string()),
                place.cpu_hours_24h.map_or("?".into(), |hours| format!("{hours:.1}")),
                place.measure
            )
        })
        .collect()
}

fn landing(snapshot: &Snapshot) -> Vec<String> {
    if snapshot.landing.is_empty() {
        return vec!["<li class=ok>No landing lock held, nobody waiting.</li>".into()];
    }
    snapshot
        .landing
        .iter()
        .map(|queue| {
            let held = queue.held_minutes.unwrap_or(0.0);
            let holder = queue.holder.as_ref().map_or("free".to_owned(), |holder| {
                format!("held {held:.0} min by {}", escape(holder.session.as_deref().unwrap_or(&holder.dir)))
            });
            let class = if held > 15.0 { "bad" } else { "" };
            format!("<li>{}: <span class={class}>{holder}</span>, {} waiting</li>", escape(&queue.repo), queue.waiters.len())
        })
        .collect()
}

fn bets(snapshot: &Snapshot) -> Vec<String> {
    snapshot
        .bets
        .iter()
        .map(|bet| {
            format!(
                "<li>Bet {} {}: {} = <b>{}</b> {} <span class=dim>({})</span></li>",
                bet.bet,
                escape(bet.name),
                escape(bet.metric),
                bet.value.map_or("?".into(), |value| format!("{value:.1}")),
                bet.unit,
                escape(&bet.detail)
            )
        })
        .collect()
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}
