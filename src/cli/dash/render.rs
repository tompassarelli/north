//! The dash as text: four sections, each headed by what it shows right now, in aligned columns with failing bars
//! first, at most `LINES` lines of at most `COLUMNS` characters, so it fits one terminal screen. On a terminal the
//! pass, fail and warning marks are coloured.
use super::Screen;
use super::billing::{self, Billing, Machine};
use super::dispatch::{Mixes, Row};
use super::ready::Verdict;
use super::spend::Spend;
use crate::foundation::clock::Timestamp;
use crate::usage::outlook::{self, Runway};

const LINES: usize = 50;
const COLUMNS: usize = 120;
const SPEND_ROWS: usize = 5;
const HABIT_ROWS: usize = 5;
const NAMED_FAILURES: usize = 1;
const FEATURE: &str = "feature";
/// The lines the workers table needs to show a few rows, below which the passing targets fold into one line.
const WORKERS_MIN: usize = 9;

#[derive(Clone, Copy)]
enum Align {
    Left,
    Right,
}

#[derive(Clone, Copy, PartialEq)]
enum Tone {
    Good,
    Bad,
    Warn,
}

impl Tone {
    fn code(self) -> &'static str {
        match self {
            Self::Good => "32",
            Self::Bad => "31",
            Self::Warn => "33",
        }
    }
}

/// The characters `from..to` of line `line` painted in `tone` when the output is a terminal.
struct Mark {
    line: usize,
    from: usize,
    to: usize,
    tone: Tone,
}

#[derive(Default)]
struct Section {
    lines: Vec<String>,
    marks: Vec<Mark>,
}

impl Section {
    fn headed(title: &str, summary: String) -> Self {
        Self { lines: vec![title.to_owned(), format!("  {summary}")], marks: Vec::new() }
    }

    fn push(&mut self, line: String) {
        self.lines.push(line);
    }

    fn mark_row(&mut self, line: usize, from: usize, to: usize, tone: Tone) {
        self.marks.push(Mark { line, from, to, tone });
    }

    fn unread(&mut self, line: String) {
        let at = self.lines.len();
        self.marks.push(Mark { line: at, from: 0, to: COLUMNS, tone: Tone::Warn });
        self.lines.push(line);
    }

    fn append(&mut self, other: Self) {
        let offset = self.lines.len();
        self.marks.extend(other.marks.into_iter().map(|mark| Mark { line: mark.line + offset, ..mark }));
        self.lines.extend(other.lines);
    }
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

fn percent(share: f64) -> String {
    if share > 0.0 && share < 0.5 { "<1%".to_owned() } else { format!("{share:.0}%") }
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

fn listed(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [only] => only.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

fn bars(screen: &Screen, window: &str, fold: bool) -> Section {
    let lines = match &screen.bars {
        Ok(lines) => lines,
        Err(why) => {
            let mut section = Section::headed("Key metrics", "the targets could not be read".to_owned());
            section.unread(format!("unread: {why}"));
            return section;
        }
    };
    let several = lines.iter().map(|line| line.repo).collect::<std::collections::BTreeSet<_>>().len() > 1;
    let name = |line: &super::ready::Line| if several { format!("{} {}", line.repo, line.plain) } else { line.plain.to_owned() };
    let mut ordered: Vec<&super::ready::Line> = lines.iter().collect();
    ordered.sort_by_key(|line| line.verdict);
    let missed: Vec<String> = ordered.iter().filter(|line| line.verdict == Verdict::Missed).map(|line| name(line)).collect();
    let slipping = ordered.iter().filter(|line| line.verdict == Verdict::Slipping).count();
    let named: Vec<String> = missed.iter().take(NAMED_FAILURES).cloned().chain((missed.len() > NAMED_FAILURES).then(|| format!("{} more", missed.len() - NAMED_FAILURES))).collect();
    let slipping_words = if slipping > 0 { format!("; {slipping} more slipping over the last {window}") } else { String::new() };
    let summary = if missed.is_empty() { format!("every target is met right now{slipping_words}") } else { format!("{} of {} targets missed right now, first {}{slipping_words}", missed.len(), lines.len(), listed(&named)) };
    let mut section = Section::headed("Key metrics", summary);
    let mut rows = vec![header(&["", "measure", "target", "last 5 min", "last 15 min", "last 1 h", &format!("last {window}")])];
    let first = section.lines.len();
    let (shown, met): (Vec<&super::ready::Line>, Vec<&super::ready::Line>) = ordered.iter().copied().partition(|line| !fold || line.verdict != Verdict::Met);
    rows.extend(shown.iter().map(|line| {
        let mark = match line.verdict {
            Verdict::Missed => "FAIL",
            Verdict::Slipping => "warn",
            Verdict::Met => "pass",
        };
        vec![mark.to_owned(), name(line), line.target.clone(), line.five_text.clone(), line.fifteen_text.clone(), line.hour_text.clone(), line.window_text.clone()]
    }));
    for (index, line) in shown.iter().enumerate() {
        let tone = match line.verdict {
            Verdict::Missed => Tone::Bad,
            Verdict::Slipping => Tone::Warn,
            Verdict::Met => Tone::Good,
        };
        section.mark_row(first + 1 + index, 0, 4, tone);
    }
    grid(&rows, &[Align::Left; 7]).into_iter().for_each(|line| section.push(line));
    if !met.is_empty() {
        section.mark_row(section.lines.len(), 0, 4, Tone::Good);
        let names: Vec<String> = met.iter().map(|line| name(line)).collect();
        section.push(format!("pass  {} more met now and over the last {window}: {}", met.len(), names.join(", ")));
    }
    section
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

fn accounts(screen: &Screen, billing: &Result<Billing, String>) -> Section {
    let mut section = Section::default();
    let accounts = match &screen.accounts {
        Ok(accounts) => accounts,
        Err(why) => {
            section.unread(format!("accounts unread: {why}"));
            return section;
        }
    };
    let mut rows = vec![header(&["account", "plan", "week left", "use per hour", "outlook", "week resets"])];
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
    for (index, account) in accounts.iter().enumerate() {
        match account.runway {
            Runway::Out => section.mark_row(index + 1, 0, COLUMNS, Tone::Bad),
            Runway::Empties { .. } => section.mark_row(index + 1, 0, COLUMNS, Tone::Warn),
            Runway::Lasts | Runway::Unknown => {}
        }
    }
    grid(&rows, &[Align::Left, Align::Left, Align::Right, Align::Right, Align::Left, Align::Left]).into_iter().for_each(|line| section.push(line));
    section
}

fn spend(spend: &Result<Spend, String>, window: &str) -> Section {
    let mut section = Section::default();
    let spend = match spend {
        Ok(spend) => spend,
        Err(why) => {
            section.unread(format!("spend unread: {why}"));
            return section;
        }
    };
    section.push(format!("Week used by the last {window} of runs, per subscription (estimated), with their API-equivalent dollars:"));
    let mut head = vec!["model and effort".to_owned()];
    head.extend(spend.accounts.iter().map(|account| format!("account {account}")));
    head.extend(["all subscriptions".to_owned(), "API-equivalent".to_owned()]);
    let mut rows = vec![head];
    let row = |label: String, classes: &[&super::spend::Class]| {
        let usd: f64 = classes.iter().map(|class| class.usd).sum();
        let mut cells = vec![label];
        cells.extend(spend.accounts.iter().map(|account| {
            let sized: Vec<f64> = classes.iter().filter_map(|class| class.week_pct.get(account).copied()).collect();
            if sized.is_empty() { "-".to_owned() } else { percent(sized.iter().sum()) }
        }));
        let all: Vec<f64> = classes.iter().filter_map(|class| class.all_pct).collect();
        cells.push(if all.is_empty() { "-".to_owned() } else { percent(all.iter().sum()) });
        cells.push(money(usd));
        cells
    };
    let shown = spend.classes.len().min(SPEND_ROWS);
    rows.extend(spend.classes[..shown].iter().map(|class| row(class.mix.clone(), &[class])));
    let rest: Vec<&super::spend::Class> = spend.classes[shown..].iter().collect();
    if !rest.is_empty() {
        rows.push(row(format!("{} more", rest.len()), &rest));
    }
    rows.push(row("all runs".to_owned(), &spend.classes.iter().collect::<Vec<_>>()));
    let mut align = vec![Align::Left];
    align.extend(std::iter::repeat_n(Align::Right, rows[0].len() - 1));
    grid(&rows, &align).into_iter().for_each(|line| section.push(line));
    section.push(format!("API credit (runs billed to the API organizations' promotional credit), last {window}: {} over {} runs", money(spend.api.usd), spend.api.runs));
    section
}

fn grouped(machines: &[&Machine]) -> Vec<(String, f64, String, usize)> {
    let mut groups: Vec<(String, f64, String, usize)> = Vec::new();
    for machine in machines {
        let billing = rate(machine);
        match groups.iter_mut().find(|group| group.2 == billing && group.1 == machine.monthly_usd) {
            Some(group) => {
                group.0 = format!("{}, {}", group.0, machine.id);
                group.3 += 1;
            }
            None => groups.push((machine.id.clone(), machine.monthly_usd, billing, 1)),
        }
    }
    groups
}

fn machines(billing: &Result<Billing, String>) -> Section {
    let mut section = Section::default();
    let billing = match billing {
        Ok(billing) => billing,
        Err(why) => {
            section.unread(format!("machines unread: {why}"));
            return section;
        }
    };
    let (paid, free): (Vec<&Machine>, Vec<&Machine>) = billing.machines.iter().partition(|machine| machine.monthly_usd > 0.0);
    let mut rows = vec![header(&["machine", "per month", "billing"])];
    rows.extend(grouped(&paid).into_iter().map(|(ids, usd, billing, count)| vec![ids, format!("{}{}", money(usd), if count > 1 { " each" } else { "" }), billing]));
    let up_to = if billing.capped() { "up to " } else { "" };
    rows.push(vec!["total".to_owned(), format!("{up_to}{}", money(billing.total_usd())), format!("not listed, $0.00: {}", free.iter().map(|machine| machine.id.as_str()).collect::<Vec<_>>().join(", "))]);
    grid(&rows, &[Align::Left, Align::Right, Align::Left]).into_iter().for_each(|line| section.push(line));
    section
}

fn billing_summary(screen: &Screen, spend: &Result<Spend, String>, window: &str) -> String {
    let accounts = match &screen.accounts {
        Ok(accounts) => {
            let short = accounts.iter().filter(|account| matches!(account.runway, Runway::Out | Runway::Empties { .. })).count();
            if short == 0 { "every account lasts to its reset".to_owned() } else { format!("{short} of {} accounts run out before reset", accounts.len()) }
        }
        Err(_) => "the accounts could not be read".to_owned(),
    };
    let money_words = match spend {
        Ok(spend) => {
            let usd: f64 = spend.classes.iter().map(|class| class.usd).sum();
            format!("{} used in {window} (+{} API credit)", money(usd), money(spend.api.usd))
        }
        Err(_) => "the spend could not be read".to_owned(),
    };
    let infra = match &screen.billing {
        Ok(billing) => format!("machines {}{} a month", if billing.capped() { "up to " } else { "" }, money(billing.total_usd())),
        Err(_) => "the machines' cost could not be read".to_owned(),
    };
    format!("{accounts}; {money_words}; {infra}")
}

fn billing(screen: &Screen, window: &str) -> Section {
    let spent = Spend::of(screen);
    let mut section = Section::headed(&format!("Billing: Claude plans, spend and machines; infra read from {}", billing::path().display()), billing_summary(screen, &spent, window));
    section.append(accounts(screen, &screen.billing));
    section.append(spend(&spent, window));
    section.append(machines(&screen.billing));
    section
}

fn first_try(landed: usize, tries: usize) -> String {
    match tries {
        0 => "-".to_owned(),
        tries => format!("{landed} of {tries} ({:.0}%)", landed as f64 / tries as f64 * 100.0),
    }
}

fn reworked(row: &Row) -> String {
    match row.closed {
        0 => "-".to_owned(),
        closed => format!("{} of {closed}", row.reworked),
    }
}

fn closed_box_cost(row: &Row) -> String {
    row.usd_per_closed.map_or_else(|| "-".to_owned(), |usd| format!("{}{}", money(usd), if row.most_per_box { " most per box" } else { "" }))
}

fn features(rows: &[&Row]) -> Section {
    let mut section = Section::headed(
        "Features, the routing decision",
        "per model and effort, the cost of each first-try landing and the closed boxes reworked within 48 h of landing".to_owned(),
    );
    let mut table = vec![header(&["model and effort", "runs", "spend", "first try landed", "cost each", "closed", "rework 48 h", "per closed box"])];
    table.extend(rows.iter().map(|row| {
        vec![
            row.mix.clone(),
            row.runs.to_string(),
            money(row.usd),
            first_try(row.first_tries_landed, row.first_tries),
            row.usd_per_first_try_landed.map_or_else(|| "-".to_owned(), money),
            row.closed.to_string(),
            reworked(row),
            closed_box_cost(row),
        ]
    }));
    grid(&table, &[Align::Left, Align::Right, Align::Right, Align::Right, Align::Right, Align::Right, Align::Left, Align::Left]).into_iter().for_each(|line| section.push(line));
    section
}

fn cells(rows: &[super::dispatch::CellRow]) -> Section {
    let mut section = Section::default();
    section.push("Cells: each category's live arms, with this window's runs, first tries and share".to_owned());
    let mut table = vec![header(&["category", "cell", "role", "runs", "first try landed", "share"])];
    let mut previous = "";
    for row in rows {
        let category = if row.category == previous { String::new() } else { row.category.clone() };
        previous = &row.category;
        table.push(vec![category, row.cell.clone(), row.role.clone(), row.runs.to_string(), first_try(row.first_tries_landed, row.first_tries), percent(100.0 * row.share)]);
    }
    grid(&table, &[Align::Left, Align::Left, Align::Left, Align::Right, Align::Right, Align::Right]).into_iter().for_each(|line| section.push(line));
    section
}

fn workers(screen: &Screen, mixes: &Result<Mixes, String>, window: &str, room: usize) -> Section {
    let title = format!("Workers: runs started in the last {window} by task category, and the boxes they closed in it");
    let mixes = match mixes {
        Ok(mixes) => mixes,
        Err(why) => {
            let mut section = Section::headed(&title, "the worker runs could not be read".to_owned());
            section.unread(format!("unread: {why}"));
            return section;
        }
    };
    let (runs, usd, tries, landed): (usize, f64, usize, usize) = mixes.rows.iter().fold((0, 0.0, 0, 0), |sum, row| (sum.0 + row.runs, sum.1 + row.usd, sum.2 + row.first_tries, sum.3 + row.first_tries_landed));
    let per_box = if mixes.closed > 0 { format!("{} each", money(usd / mixes.closed as f64)) } else { "none closed".to_owned() };
    let first = if tries > 0 { format!("; {:.0}% of first tries landed", landed as f64 / tries as f64 * 100.0) } else { String::new() };
    let mut section = Section::headed(&title, format!("{runs} worker runs closed {} boxes for {} ({per_box}){first}", mixes.closed, money(usd)));
    if let Err(why) = &screen.attempts {
        section.unread(format!("first tries unread: {why}"));
    }
    let (feature, others): (Vec<&Row>, Vec<&Row>) = mixes.rows.iter().partition(|row| row.category == FEATURE);
    if !feature.is_empty() {
        section.append(features(&feature));
    }
    let room = room.saturating_sub(section.lines.len() + 2);
    let shown = if others.len() > room { room.saturating_sub(1) } else { others.len() };
    let mut rows = vec![header(&["category", "model and effort", "runs", "spend", "first try landed", "closed", "per closed box"])];
    let mut previous = "";
    for row in &others[..shown] {
        let category = if row.category == previous { String::new() } else { row.category.clone() };
        previous = &row.category;
        rows.push(vec![category, row.mix.clone(), row.runs.to_string(), money(row.usd), first_try(row.first_tries_landed, row.first_tries), row.closed.to_string(), closed_box_cost(row)]);
    }
    let rest = &others[shown..];
    if !rest.is_empty() {
        rows.push(vec![format!("{} more model and effort rows", rest.len()), String::new(), rest.iter().map(|row| row.runs).sum::<usize>().to_string(), money(rest.iter().map(|row| row.usd).sum())]);
    }
    let per_closed = if mixes.closed > 0 { money(usd / mixes.closed as f64) } else { "-".to_owned() };
    rows.push(vec!["all categories".to_owned(), String::new(), runs.to_string(), money(usd), first_try(landed, tries), mixes.closed.to_string(), per_closed]);
    grid(&rows, &[Align::Left, Align::Left, Align::Right, Align::Right, Align::Right, Align::Right, Align::Left]).into_iter().for_each(|line| section.push(line));
    if !mixes.cells.is_empty() {
        section.push(String::new());
        section.append(cells(&mixes.cells));
    }
    section
}

fn habits(screen: &Screen, window: &str) -> Section {
    let title = format!("Habits: what the edit hook rewrote or warned about in .rs files in the last {window}, by rule");
    let rows = match &screen.habits {
        Ok(rows) => rows,
        Err(why) => {
            let mut section = Section::headed(&title, "the habit events could not be read".to_owned());
            section.unread(format!("unread: {why}"));
            return section;
        }
    };
    let (rewrites, warnings) = rows.iter().fold((0, 0), |(rewrites, warnings), row| (rewrites + row.rewrites, warnings + row.warnings));
    let count = |number: usize, word: &str| format!("{number} {word}{}", if number == 1 { "" } else { "s" });
    let summary = if rows.is_empty() {
        format!("no habit rewrite or warning in the last {window}")
    } else {
        format!("{} and {} on {} {}", count(rewrites, "rewrite"), count(warnings, "warning"), rows.len(), if rows.len() == 1 { "rule" } else { "rules" })
    };
    let mut section = Section::headed(&title, summary);
    if rows.is_empty() {
        return section;
    }
    let shown = rows.len().min(HABIT_ROWS);
    let mut table = vec![header(&["rule", "rewrites", "warnings"])];
    table.extend(rows[..shown].iter().map(|row| vec![row.rule.clone(), row.rewrites.to_string(), row.warnings.to_string()]));
    grid(&table, &[Align::Left, Align::Right, Align::Right]).into_iter().for_each(|line| section.push(line));
    if rows.len() > shown {
        section.push(format!("{} more rules", rows.len() - shown));
    }
    section
}

fn paint(line: &str, marks: &[&Mark]) -> String {
    let characters: Vec<char> = line.chars().collect();
    let mut out = String::new();
    let mut at = 0;
    let mut ordered: Vec<&&Mark> = marks.iter().collect();
    ordered.sort_by_key(|mark| mark.from);
    for mark in ordered {
        let (from, to) = (mark.from.min(characters.len()), mark.to.min(characters.len()));
        if from < at || from >= to {
            continue;
        }
        out.extend(&characters[at..from]);
        out.push_str(&format!("\x1b[{}m", mark.tone.code()));
        out.extend(&characters[from..to]);
        out.push_str("\x1b[0m");
        at = to;
    }
    out.extend(&characters[at..]);
    out
}

pub fn screen(screen: &Screen, color: bool) -> String {
    let window = window_words(&screen.window);
    let mixes = Mixes::of(screen);
    let mut all = Section::default();
    all.push(format!("north dash: the last {window}, read at {}", Timestamp::from_unix_seconds(screen.now).local_twelve_hour()));
    all.push(String::new());
    let billed = billing(screen, &window);
    let others = all.lines.len() + billed.lines.len() + 2;
    let unfolded = bars(screen, &window, false);
    let metrics = if LINES.saturating_sub(others + unfolded.lines.len()) < WORKERS_MIN { bars(screen, &window, true) } else { unfolded };
    all.append(metrics);
    all.push(String::new());
    all.append(billed);
    all.push(String::new());
    let room = LINES.saturating_sub(all.lines.len());
    all.append(workers(screen, &mixes, &window, room));
    all.push(String::new());
    all.append(habits(screen, &window));
    all.lines
        .iter()
        .enumerate()
        .take(LINES)
        .map(|(index, line)| {
            let line: String = line.chars().take(COLUMNS).collect();
            let marks: Vec<&Mark> = all.marks.iter().filter(|mark| mark.line == index).collect();
            let text = if color && !marks.is_empty() { paint(&line, &marks) } else { line };
            format!("{text}\n")
        })
        .collect()
}
