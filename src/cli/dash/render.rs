//! The dash as text: three sections in aligned columns, failing bars first, at most `LINES` lines of at most 120
//! characters, so it fits one terminal screen.
use std::collections::BTreeMap;

use super::Screen;
use super::billing::{self, Billing, Machine};
use super::dispatch::Mixes;
use crate::foundation::clock::Timestamp;
use crate::usage::outlook::{self, Runway};

const LINES: usize = 50;

#[derive(Clone, Copy)]
enum Align {
    Left,
    Right,
}

fn grid(rows: &[Vec<String>], align: &[Align]) -> Vec<String> {
    let widths: Vec<usize> = (0..align.len()).map(|column| rows.iter().map(|row| row.get(column).map_or(0, |cell| cell.chars().count())).max().unwrap_or(0)).collect();
    rows.iter()
        .map(|row| {
            let cells: Vec<String> = row.iter().zip(&widths).zip(align).map(|((cell, width), align)| match align {
                Align::Left => format!("{cell:<width$}"),
                Align::Right => format!("{cell:>width$}"),
            }).collect();
            cells.join("  ").trim_end().to_owned()
        })
        .collect()
}

fn header(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| (*word).to_owned()).collect()
}

fn money(usd: f64) -> String {
    format!("${usd:.2}")
}

fn span(minutes: f64) -> String {
    let minutes = minutes.round() as i64;
    match minutes {
        minutes if minutes < 60 => format!("{minutes} min"),
        minutes if minutes % 60 == 0 => format!("{} h", minutes / 60),
        minutes => format!("{} h {} min", minutes / 60, minutes % 60),
    }
}

fn window_words(window: &str) -> String {
    let split = window.find(|character: char| !character.is_ascii_digit()).unwrap_or(window.len());
    let (count, unit) = window.split_at(split);
    match unit {
        "d" if count == "1" => "1 day".to_owned(),
        "d" => format!("{count} days"),
        _ => format!("{count} {unit}"),
    }
}

fn bars(screen: &Screen, window: &str) -> Vec<String> {
    let lines = match &screen.bars {
        Ok(lines) => lines,
        Err(why) => return vec![format!("Key metrics: unread: {why}")],
    };
    let mut repos: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for line in lines {
        let counted = repos.entry(line.repo).or_default();
        counted.0 += usize::from(!line.passed);
        counted.1 += 1;
    }
    let several = repos.len() > 1;
    let heads: Vec<String> = repos.iter().map(|(repo, (failing, of))| format!("{failing} of {of} bars fail on the last 1 h{}", if several { format!(" in {repo}") } else { String::new() })).collect();
    let mut out = vec![format!("Key metrics: {}", heads.join("; "))];
    let mut ordered: Vec<&super::ready::Line> = lines.iter().collect();
    ordered.sort_by_key(|line| line.passed);
    let mut rows = vec![header(&["", "bar", "target", "last 1 h", &format!("last {window}")])];
    rows.extend(ordered.iter().map(|line| {
        let name = if several { format!("{} {}", line.repo, line.bar) } else { line.bar.to_owned() };
        vec![if line.passed { "pass" } else { "FAIL" }.to_owned(), name, line.target.clone(), line.hour_text.clone(), line.window_text.clone()]
    }));
    out.extend(grid(&rows, &[Align::Left; 5]));
    out
}

fn runway(runway: Runway) -> String {
    match runway {
        Runway::Out => "out until reset".to_owned(),
        Runway::Empties { minutes } => format!("runs out in {}", span(minutes)),
        Runway::Lasts => "lasts to reset".to_owned(),
        Runway::Unknown => "no burn rate yet".to_owned(),
    }
}

fn rate(machine: &Machine) -> String {
    match (machine.hourly_eur, machine.monthly_cap_eur) {
        (Some(hourly), Some(cap)) => format!("€{hourly}/h, capped at €{cap}/month"),
        (Some(hourly), None) => format!("€{hourly}/h"),
        _ => "fixed".to_owned(),
    }
}

fn accounts(screen: &Screen, billing: &Result<Billing, String>) -> Vec<String> {
    let accounts = match &screen.accounts {
        Ok(accounts) => accounts,
        Err(why) => return vec![format!("accounts unread: {why}")],
    };
    let mut rows = vec![header(&["account", "plan", "week left", "burn", "outlook", "week resets"])];
    rows.extend(accounts.iter().map(|account| {
        let plan = billing.as_ref().ok().and_then(|billing| billing.plans.get(&account.account));
        vec![
            account.account.clone(),
            plan.map_or_else(|| "-".to_owned(), |plan| format!("{} {}/month", plan.plan, money(plan.monthly_usd))),
            account.week_used.map_or_else(|| "-".to_owned(), |used| format!("{:.0}%", (100.0 - used).max(0.0))),
            account.burn_per_hour.map_or_else(|| "-".to_owned(), |rate| format!("{rate:.1}%/h")),
            runway(account.runway),
            account.week_resets.map_or_else(|| "-".to_owned(), outlook::weekday_clock),
        ]
    }));
    grid(&rows, &[Align::Left, Align::Left, Align::Right, Align::Right, Align::Left, Align::Left])
}

fn spend(mixes: &Result<Mixes, String>, window: &str) -> String {
    match mixes {
        Ok(mixes) => {
            let total: f64 = mixes.models.iter().map(|spend| spend.usd).sum();
            let by_model: Vec<String> = mixes.models.iter().map(|spend| format!("{} {}", spend.mix, money(spend.usd))).collect();
            format!("API-equivalent spend, last {window}: {}; all {} over {} runs", by_model.join(", "), money(total), mixes.models.iter().map(|spend| spend.runs).sum::<usize>())
        }
        Err(why) => format!("spend unread: {why}"),
    }
}

fn machines(billing: &Result<Billing, String>) -> Vec<String> {
    let billing = match billing {
        Ok(billing) => billing,
        Err(why) => return vec![format!("machines unread: {why}")],
    };
    let (paid, free): (Vec<&Machine>, Vec<&Machine>) = billing.machines.iter().partition(|machine| machine.monthly_usd > 0.0);
    let mut rows = vec![header(&["machine", "per month", "billing"])];
    rows.extend(paid.iter().map(|machine| vec![machine.id.clone(), money(machine.monthly_usd), rate(machine)]));
    let up_to = if billing.capped() { "up to " } else { "" };
    rows.push(vec!["total".to_owned(), format!("{up_to}{}", money(billing.total_usd())), format!("not listed, $0.00: {}", free.iter().map(|machine| machine.id.as_str()).collect::<Vec<_>>().join(", "))]);
    grid(&rows, &[Align::Left, Align::Right, Align::Left])
}

fn billing(screen: &Screen, mixes: &Result<Mixes, String>, window: &str) -> Vec<String> {
    let mut out = vec![format!("Billing: Claude plans, spend and machines; infra read from {}", billing::path().display())];
    out.extend(accounts(screen, &screen.billing));
    out.push(spend(mixes, window));
    out.extend(machines(&screen.billing));
    out
}

fn first_try(landed: usize, tries: usize) -> String {
    match tries {
        0 => "-".to_owned(),
        tries => format!("{landed} of {tries} ({:.0}%)", landed as f64 / tries as f64 * 100.0),
    }
}

fn dispatch(screen: &Screen, mixes: &Result<Mixes, String>, window: &str, room: usize) -> Vec<String> {
    let mut out = vec![format!("Dispatch: worker runs started in the last {window} by task category, and the boxes they closed in it")];
    let mixes = match mixes {
        Ok(mixes) => mixes,
        Err(why) => return vec![format!("{}: unread: {why}", out[0])],
    };
    if let Err(why) = &screen.attempts {
        out.push(format!("first tries unread: {why}"));
    }
    let room = room.saturating_sub(out.len() + 2);
    let shown = if mixes.rows.len() > room { room.saturating_sub(1) } else { mixes.rows.len() };
    let mut rows = vec![header(&["category", "model and effort", "runs", "spend", "first try landed", "closed", "per closed box"])];
    let mut previous = "";
    for row in &mixes.rows[..shown] {
        let category = if row.category == previous { String::new() } else { row.category.clone() };
        previous = &row.category;
        let per_closed = row.usd_per_closed.map_or_else(|| "-".to_owned(), |usd| format!("{}{}", money(usd), if row.dearest { " dearest" } else { "" }));
        rows.push(vec![category, row.mix.clone(), row.runs.to_string(), money(row.usd), first_try(row.first_tries_landed, row.first_tries), row.closed.to_string(), per_closed]);
    }
    let rest = &mixes.rows[shown..];
    if !rest.is_empty() {
        rows.push(vec![format!("{} more mixes", rest.len()), String::new(), rest.iter().map(|row| row.runs).sum::<usize>().to_string(), money(rest.iter().map(|row| row.usd).sum())]);
    }
    let (runs, usd, tries, landed): (usize, f64, usize, usize) = mixes.rows.iter().fold((0, 0.0, 0, 0), |sum, row| (sum.0 + row.runs, sum.1 + row.usd, sum.2 + row.first_tries, sum.3 + row.first_tries_landed));
    let per_closed = if mixes.closed > 0 { money(usd / mixes.closed as f64) } else { "-".to_owned() };
    rows.push(vec!["all categories".to_owned(), String::new(), runs.to_string(), money(usd), first_try(landed, tries), mixes.closed.to_string(), per_closed]);
    out.extend(grid(&rows, &[Align::Left, Align::Left, Align::Right, Align::Right, Align::Right, Align::Right, Align::Left]));
    out
}

pub fn screen(screen: &Screen) -> String {
    let window = window_words(&screen.window);
    let mixes = Mixes::of(screen);
    let mut out = vec![format!("north dash: the last {window}, read at {}", Timestamp::from_unix_seconds(screen.now).local_twelve_hour()), String::new()];
    out.extend(bars(screen, &window));
    out.push(String::new());
    out.extend(billing(screen, &mixes, &window));
    out.push(String::new());
    let room = LINES.saturating_sub(out.len());
    out.extend(dispatch(screen, &mixes, &window, room));
    out.iter().map(|line| format!("{line}\n")).collect()
}
