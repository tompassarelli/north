//! `north threads summary`: runs by cell and tier, their context and delivery figures, lenses, splits and closure.
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

use rusqlite::{Connection, Row};

use super::boxes::{BoxRun, box_groups, moment};
use super::read::escalated;
use super::{GROUPS, Knobs, cell_value, median, tier_rank};
use crate::threads::board::issues::Board;
use crate::threads::catalog::CATALOG;
use crate::threads::verb::{Args, Context, Failure, open, report, table};

const UNTRACKED: &str = "(untracked)";

struct Run {
    agent: String,
    item: Option<String>,
    follows: Option<String>,
    tier: String,
    cells: [String; 5],
    started: Option<String>,
    ended: String,
    minutes: i64,
    eta_min: Option<i64>,
    tokens: Option<i64>,
    peak_ctx: Option<i64>,
    outcome: String,
    landed: Option<i64>,
    arm: Option<String>,
    lens: Option<String>,
    flaws: Option<i64>,
    expects_landing: Option<i64>,
    staffed: Option<String>,
    review_requested: Option<String>,
    landed_at: Option<String>,
    role: String,
    category: String,
    compactions: Option<i64>,
    handoff_fires: Option<i64>,
    input_tokens: Option<i64>,
    cache_read_tokens: Option<i64>,
    idle_gap_misses: Option<i64>,
    cost_usd: Option<f64>,
    model: Option<String>,
    effort: Option<String>,
    fast: Option<i64>,
}

impl Run {
    fn read(row: &Row) -> rusqlite::Result<Self> {
        let cell = |key: &str| -> rusqlite::Result<String> { Ok(cell_value(key, row.get::<_, Option<String>>(key)?.as_deref())) };
        Ok(Self {
            agent: row.get("agent")?,
            item: row.get("item")?,
            follows: row.get("follows")?,
            tier: row.get("tier")?,
            cells: [cell("category")?, cell("spec")?, cell("scope")?, cell("surface")?, cell("verify")?],
            started: row.get("started")?,
            ended: row.get("ended")?,
            minutes: row.get("minutes")?,
            eta_min: row.get("eta_min")?,
            tokens: row.get("tokens")?,
            peak_ctx: row.get("peak_ctx")?,
            outcome: row.get("outcome")?,
            landed: row.get("landed")?,
            arm: row.get("arm")?,
            lens: row.get("lens")?,
            flaws: row.get("flaws")?,
            expects_landing: row.get("expects_landing")?,
            staffed: row.get("staffed")?,
            review_requested: row.get("review_requested")?,
            landed_at: row.get("landed_at")?,
            role: row.get("role")?,
            category: row.get("category")?,
            compactions: row.get("compactions")?,
            handoff_fires: row.get("handoff_fires")?,
            input_tokens: row.get("input_tokens")?,
            cache_read_tokens: row.get("cache_read_tokens")?,
            idle_gap_misses: row.get("idle_gap_misses")?,
            cost_usd: row.get("cost_usd")?,
            model: row.get("model")?,
            effort: row.get("effort")?,
            fast: row.get("fast")?,
        })
    }

    fn unjudged(&self) -> bool {
        matches!(self.outcome.as_str(), "running" | "idle") || (self.outcome == "unclear" && self.item.as_deref().is_none_or(str::is_empty))
    }

    fn succeeded(&self, followed: bool) -> bool {
        self.outcome == "done" && !followed && !(self.expects_landing.is_some_and(|expects| expects != 0) && self.landed == Some(0))
    }

    fn box_run(&self) -> BoxRun {
        BoxRun {
            item: self.item.clone().unwrap_or_default(),
            category: self.category.clone(),
            outcome: self.outcome.clone(),
            staffed: self.staffed.clone(),
            started: self.started.clone(),
            ended: self.ended.clone(),
            landed: self.landed.is_some_and(|landed| landed != 0),
            landed_at: self.landed_at.clone(),
            cost_usd: self.cost_usd,
        }
    }
}

fn medians(values: impl IntoIterator<Item = i64>) -> Option<i64> {
    let values: Vec<f64> = values.into_iter().map(|value| value as f64).collect();
    median(&values).map(|value| value as i64)
}

fn dash(value: Option<i64>) -> String {
    value.map_or_else(|| "-".to_owned(), |value| value.to_string())
}

fn span(start: Option<&str>, end: Option<&str>) -> Option<i64> {
    let (start, end) = (start.filter(|text| !text.is_empty())?, end.filter(|text| !text.is_empty())?);
    let nanos = moment(end)?.unix_nanos() - moment(start)?.unix_nanos();
    Some(nanos.div_euclid(60_000_000_000).max(0) as i64)
}

fn percentile(values: &[i64], fraction: f64) -> String {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    let index = ((sorted.len() as f64 * fraction).ceil() as usize).saturating_sub(1);
    sorted.get(index).map_or_else(|| "-".to_owned(), i64::to_string)
}

fn strings(row: &[&str]) -> Vec<String> {
    row.iter().map(|cell| (*cell).to_owned()).collect()
}

fn context_summary(rows: &[Run]) -> String {
    let mut groups: BTreeMap<(&str, &str, &str), Vec<&Run>> = BTreeMap::new();
    for run in rows {
        groups.entry((&run.role, &run.category, &run.tier)).or_default().push(run);
    }
    let mut out = vec![strings(&["role", "category", "tier", "n", "peak_ctx_p50", "peak_ctx_p90", "compactions/run", "handoff_fires", "cache_read_share", "idle_gap_misses"])];
    for ((role, category, tier), runs) in groups {
        let measured: Vec<&Run> = runs.iter().copied().filter(|run| run.compactions.is_some()).collect();
        let inputs: i64 = measured.iter().map(|run| run.input_tokens.unwrap_or(0)).sum();
        let cache: i64 = measured.iter().map(|run| run.cache_read_tokens.unwrap_or(0)).sum();
        let peaks: Vec<i64> = measured.iter().filter_map(|run| run.peak_ctx).collect();
        let total = |pick: fn(&Run) -> Option<i64>| if measured.is_empty() { "-".to_owned() } else { measured.iter().map(|run| pick(run).unwrap_or(0)).sum::<i64>().to_string() };
        out.push(vec![
            role.to_owned(),
            category.to_owned(),
            tier.to_owned(),
            runs.len().to_string(),
            percentile(&peaks, 0.5),
            percentile(&peaks, 0.9),
            if measured.is_empty() { "-".to_owned() } else { format!("{:.2}", measured.iter().map(|run| run.compactions.unwrap_or(0)).sum::<i64>() as f64 / measured.len() as f64) },
            total(|run| run.handoff_fires),
            if inputs == 0 { "-".to_owned() } else { format!("{:.1}%", cache as f64 / inputs as f64 * 100.0) },
            total(|run| run.idle_gap_misses),
        ]);
    }
    table(&out)
}

struct Delivery {
    done_boxes: usize,
    concurrency: i64,
    brief_to_landed_min: Option<f64>,
    cost_per_done_usd: Option<f64>,
}

fn statistics_median(values: &mut [f64]) -> Option<f64> {
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    match values.len() {
        0 => None,
        length if length % 2 == 1 => Some(values[middle]),
        _ => Some((values[middle - 1] + values[middle]) / 2.0),
    }
}

fn begin(run: &Run) -> Option<&str> {
    run.staffed.as_deref().filter(|text| !text.is_empty()).or(run.started.as_deref()).filter(|text| !text.is_empty())
}

fn delivery(knobs: &Knobs, rows: &[&Run], project: Option<&str>) -> Delivery {
    let rows: Vec<&Run> = rows.iter().copied().filter(|run| project.is_none_or(|project| run.item.as_deref().unwrap_or("").split('#').next() == Some(project))).collect();
    let box_runs: Vec<BoxRun> = rows.iter().map(|run| run.box_run()).collect();
    let mut costs: Vec<f64> = box_groups(&box_runs, knobs.box_chain_gap_min)
        .iter()
        .filter(|group| group.iter().all(|(_, run)| run.cost_usd.is_some()))
        .map(|group| group.iter().filter_map(|(_, run)| run.cost_usd).sum())
        .collect();
    let done: Vec<&Run> = rows.iter().copied().filter(|run| run.role == "worker" && run.outcome == "done" && run.landed == Some(1)).collect();
    let mut events: Vec<(i128, i64)> = Vec::new();
    for run in &done {
        if let (Some(start), Some(end)) = (begin(run).and_then(moment), moment(&run.ended)) {
            events.extend([(start.unix_nanos(), 1), (end.unix_nanos(), -1)]);
        }
    }
    events.sort_unstable();
    let (mut active, mut concurrency) = (0i64, 1i64);
    for (_, change) in events {
        active += change;
        concurrency = concurrency.max(active);
    }
    let followed: BTreeSet<Option<&str>> = done.iter().map(|run| run.follows.as_deref()).collect();
    let done: Vec<&Run> = done.into_iter().filter(|run| !followed.contains(&Some(run.agent.as_str()))).collect();
    let mut durations: Vec<f64> = done
        .iter()
        .filter_map(|run| {
            let landed = moment(run.landed_at.as_deref().filter(|text| !text.is_empty())?)?;
            Some((landed.unix_nanos() - moment(begin(run)?)?.unix_nanos()) as f64 / 60e9)
        })
        .collect();
    Delivery { done_boxes: done.len(), concurrency, brief_to_landed_min: statistics_median(&mut durations), cost_per_done_usd: statistics_median(&mut costs) }
}

fn delivery_cells(metrics: &Delivery) -> Vec<String> {
    let fixed = |value: Option<f64>| value.map_or_else(|| "-".to_owned(), |value| format!("{value:.3}"));
    vec![metrics.done_boxes.to_string(), fixed(metrics.brief_to_landed_min), fixed(metrics.cost_per_done_usd)]
}

fn delivery_summary(knobs: &Knobs, rows: &[&Run]) -> String {
    let mut groups: BTreeMap<(String, String, String), Vec<&Run>> = BTreeMap::new();
    for run in rows {
        let key = (run.model.clone().unwrap_or_else(|| "-".into()), run.effort.clone().unwrap_or_else(|| "-".into()), run.fast.map_or_else(|| "-".into(), |fast| fast.to_string()));
        groups.entry(key).or_default().push(run);
    }
    let mut out = vec![strings(&["model", "effort", "fast", "done_boxes", "brief_to_landed_min_med", "usd_per_done_med", "concurrency"])];
    for ((model, effort, fast), runs) in groups {
        let metrics = delivery(knobs, &runs, None);
        out.push([vec![model, effort, fast], delivery_cells(&metrics), vec![metrics.concurrency.to_string()]].concat());
    }
    let mut text = table(&out);
    let projects: BTreeSet<&str> = rows.iter().filter_map(|run| run.item.as_deref().filter(|item| !item.is_empty())).map(|item| item.split('#').next().unwrap_or_default()).collect();
    let mut out = vec![strings(&["project", "done_boxes", "brief_to_landed_min_med", "usd_per_done_med", "concurrency"])];
    for project in projects {
        let metrics = delivery(knobs, rows, Some(project));
        out.push([vec![project.to_owned()], delivery_cells(&metrics), vec![metrics.concurrency.to_string()]].concat());
    }
    text += &table(&out);
    text
}

fn split_summary(splits: &[&Run], rows: &[&Run]) -> String {
    struct Tally {
        splits: usize,
        parts: usize,
        closed: usize,
        minutes: i64,
        spans: Vec<Option<i64>>,
    }
    let mut groups: BTreeMap<&str, Tally> = BTreeMap::new();
    for split in splits {
        let parts: Vec<&&Run> = rows.iter().filter(|run| run.follows.as_deref() == Some(split.agent.as_str())).collect();
        let tally = groups.entry(&split.cells[0]).or_insert(Tally { splits: 0, parts: 0, closed: 0, minutes: 0, spans: Vec::new() });
        tally.splits += 1;
        tally.parts += parts.len();
        tally.closed += parts.iter().filter(|run| run.outcome == "done").count();
        tally.minutes += parts.iter().map(|run| run.minutes).sum::<i64>();
        if let Some(last) = parts.iter().map(|run| run.ended.as_str()).max() {
            tally.spans.push(span(Some(&split.ended), Some(last)));
        }
    }
    let mut out = vec![strings(&["category", "splits", "parts", "parts_closed", "parts_min", "split_to_last_min"])];
    for (category, tally) in groups {
        out.push(vec![
            category.to_owned(),
            tally.splits.to_string(),
            tally.parts.to_string(),
            if tally.parts == 0 { "-".to_owned() } else { format!("{}%", (tally.closed as f64 * 100.0 / tally.parts as f64).round_ties_even() as i64) },
            tally.minutes.to_string(),
            if tally.spans.is_empty() { "-".to_owned() } else { tally.spans.iter().flatten().sum::<i64>().to_string() },
        ]);
    }
    table(&out)
}

fn closure(context: &Context, rows: &[&Run]) -> Result<String, Failure> {
    let mut by_item: Vec<(&str, Vec<&Run>)> = Vec::new();
    for run in rows {
        let Some(item) = run.item.as_deref().filter(|item| CATALOG.parse_ref(item).is_ok()) else { continue };
        match by_item.iter_mut().find(|(known, _)| *known == item) {
            Some((_, runs)) => runs.push(run),
            None => by_item.push((item, vec![run])),
        }
    }
    struct Group {
        issues: usize,
        closed: usize,
        runs: usize,
        tokens: i64,
        escalated: usize,
    }
    let mut board: Option<Board> = None;
    let mut opened: BTreeMap<String, BTreeSet<u64>> = BTreeMap::new();
    let mut groups: Vec<((String, String), Group)> = Vec::new();
    for (item, mut runs) in by_item {
        runs.sort_by(|left, right| left.started.as_deref().filter(|text| !text.is_empty()).unwrap_or(&left.ended).cmp(right.started.as_deref().filter(|text| !text.is_empty()).unwrap_or(&right.ended)));
        let first = runs[0];
        let (repo, number) = CATALOG.parse_ref(item).map_err(Failure::Refused)?;
        if !opened.contains_key(&repo) {
            if board.is_none() {
                board = Some(Board::open(context)?);
            }
            let numbers = board.as_ref().expect("opened above").open_issues(&repo)?.into_iter().map(|issue| issue.number).collect();
            opened.insert(repo.clone(), numbers);
        }
        let key = (first.category.clone(), first.tier.clone());
        let index = groups.iter().position(|(known, _)| *known == key).unwrap_or_else(|| {
            groups.push((key, Group { issues: 0, closed: 0, runs: 0, tokens: 0, escalated: 0 }));
            groups.len() - 1
        });
        let group = &mut groups[index].1;
        group.issues += 1;
        group.escalated += usize::from(runs[1..].iter().any(|run| tier_rank(&run.tier) > tier_rank(&first.tier)));
        if !opened[&repo].contains(&number) {
            group.closed += 1;
            group.runs += runs.len();
            group.tokens += runs.iter().map(|run| run.tokens.unwrap_or(0)).sum::<i64>();
        }
    }
    groups.sort_by(|(left, _), (right, _)| left.0.cmp(&right.0).then(tier_rank(&left.1).cmp(&tier_rank(&right.1))));
    let mut out = vec![strings(&["category", "first_tier", "issues", "closed", "runs_per_closed", "tokens_per_closed", "escalated"])];
    for ((category, tier), group) in groups {
        let closed = group.closed;
        out.push(vec![
            category,
            tier,
            group.issues.to_string(),
            closed.to_string(),
            if closed == 0 { "-".to_owned() } else { format!("{:.1}", group.runs as f64 / closed as f64) },
            if closed == 0 { "-".to_owned() } else { format!("{}k", group.tokens / closed as i64 / 1000) },
            format!("{}%", group.escalated * 100 / group.issues),
        ]);
    }
    Ok(table(&out))
}

fn group_keys(text: &str) -> Result<Vec<&str>, Failure> {
    let keys: Vec<&str> = text.split(',').map(str::trim).filter(|key| !key.is_empty()).collect();
    if keys.is_empty() || keys.iter().any(|key| !GROUPS.contains(&key.to_lowercase().as_str()) && key.to_lowercase() != "arm") {
        return Err(Failure::Refused(format!("--group takes a comma list of {}, arm; got '{text}'", GROUPS.join(", "))));
    }
    Ok(keys.into_iter().filter(|key| key.to_lowercase() != "arm").collect())
}

fn runs_table(rows: &[&Run], splits: &BTreeMap<&str, &Run>, keys: &[&str], escalated: &BTreeSet<String>) -> String {
    let columns: Vec<usize> = keys.iter().filter_map(|key| GROUPS.iter().position(|group| group.eq_ignore_ascii_case(key))).collect();
    let mut groups: Vec<(Vec<String>, Vec<&Run>)> = Vec::new();
    for run in rows.iter().filter(|run| !splits.contains_key(run.agent.as_str())) {
        let arm = run.arm.clone().filter(|arm| !arm.is_empty()).or_else(|| run.follows.as_deref().and_then(|follows| splits.get(follows)).and_then(|split| split.arm.clone()).filter(|arm| !arm.is_empty()));
        let mut key: Vec<String> = columns.iter().map(|column| run.cells[*column].clone()).collect();
        key.extend([run.tier.clone(), arm.unwrap_or_else(|| "-".into())]);
        match groups.iter_mut().find(|(known, _)| *known == key) {
            Some((_, runs)) => runs.push(run),
            None => groups.push((key, vec![run])),
        }
    }
    groups.sort_by(|(left, _), (right, _)| {
        let cells = left.len() - 2;
        left[..cells].cmp(&right[..cells]).then(tier_rank(&left[cells]).cmp(&tier_rank(&right[cells]))).then(left[cells + 1].cmp(&right[cells + 1]))
    });
    let mut out = vec![[keys.iter().map(|key| (*key).to_owned()).collect::<Vec<_>>(), strings(&["tier", "arm", "n", "done", "success", "ok", "eta_med", "actual_med", "work_med", "wait_med", "x_eta_med", "over_2x_eta", "tokens_med", "no_landing", "usd_per_done"])].concat()];
    let knobs = Knobs::registry();
    for (key, runs) in groups {
        let n = runs.len();
        let done = runs.iter().filter(|run| run.outcome == "done").count();
        let ok = runs.iter().filter(|run| run.succeeded(escalated.contains(&run.agent))).count();
        let timed: Vec<&&Run> = runs.iter().filter(|run| run.eta_min.is_some_and(|eta| eta != 0)).collect();
        let ratios: Vec<f64> = timed.iter().map(|run| run.minutes as f64 / run.eta_min.unwrap_or(1) as f64).collect();
        let spans = |from: fn(&Run) -> Option<&str>, to: fn(&Run) -> Option<&str>| dash(medians(runs.iter().filter_map(|run| span(from(run), to(run)))));
        let expecting = runs.iter().filter(|run| run.expects_landing == Some(1)).count();
        let usd = delivery(&knobs, &runs, None).cost_per_done_usd;
        out.push(
            [
                key,
                vec![
                    n.to_string(),
                    done.to_string(),
                    format!("{}%", done * 100 / n),
                    format!("{}%", ok * 100 / n),
                    medians(runs.iter().filter_map(|run| run.eta_min)).filter(|eta| *eta != 0).map_or_else(|| "-".into(), |eta| eta.to_string()),
                    dash(medians(runs.iter().map(|run| run.minutes))),
                    spans(|run| run.staffed.as_deref(), |run| run.review_requested.as_deref()),
                    spans(|run| run.review_requested.as_deref(), |run| run.landed_at.as_deref()),
                    median(&ratios).map_or_else(|| "-".into(), |ratio| format!("{ratio:.1}")),
                    format!("{}/{}", timed.iter().filter(|run| run.minutes > 2 * run.eta_min.unwrap_or(0)).count(), timed.len()),
                    medians(runs.iter().filter_map(|run| run.tokens)).map_or_else(|| "-".into(), |tokens| format!("{}k", tokens.div_euclid(1000))),
                    format!("{}/{expecting}", runs.iter().filter(|run| run.expects_landing == Some(1) && run.landed == Some(0)).count()),
                    usd.map_or_else(|| "-".into(), |usd| format!("{usd:.2}")),
                ],
            ]
            .concat(),
        );
    }
    table(&out)
}

fn has_table(db: &Connection, name: &str) -> rusqlite::Result<bool> {
    db.query_row("SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1", [name], |_| Ok(())).map(|()| true).or_else(|error| if error == rusqlite::Error::QueryReturnedNoRows { Ok(false) } else { Err(error) })
}

fn summarise(args: &Args, context: &Context, err: &mut dyn Write) -> Result<String, Failure> {
    let since = args.get("--since").unwrap_or("");
    let keys = group_keys(args.get("--group").unwrap_or("category"))?;
    let db = open(context.database, false)?;
    let mut statement = db.prepare("SELECT * FROM runs WHERE outcome != 'running' AND ended >= ?1")?;
    let all: Vec<Run> = statement.query_map([since], Run::read)?.collect::<Result<_, _>>()?;
    if all.is_empty() {
        return Err(Failure::Refused(format!("no runs in {}", context.database.display())));
    }
    let knobs = Knobs::registry();
    let escalated = escalated(&db)?;
    let rows: Vec<&Run> = all.iter().filter(|run| run.role == "worker" && !run.unjudged()).collect();
    let splits: BTreeMap<&str, &Run> = rows.iter().filter(|run| run.outcome == "split").map(|run| (run.agent.as_str(), *run)).collect();
    let mut text = runs_table(&rows, &splits, &keys, &escalated);
    text += &format!("\n{}\n{}", context_summary(&all), delivery_summary(&knobs, &rows));
    if has_table(&db, "run_costs")? {
        let mut costs = db.prepare("SELECT account, SUM(cost_usd) FROM run_costs WHERE agent = ?1 AND ended >= ?2 GROUP BY account ORDER BY account")?;
        for line in costs.query_map([UNTRACKED, since], |row| Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?)))? {
            let (account, usd) = line?;
            text += &format!("untracked sessions on {account}: ${usd:.2} unallocated\n");
        }
    }
    let mut lenses: BTreeMap<&str, Vec<&Run>> = BTreeMap::new();
    for run in &rows {
        if let Some(lens) = run.lens.as_deref().filter(|lens| !lens.is_empty()) {
            lenses.entry(lens).or_default().push(run);
        }
    }
    if !lenses.is_empty() {
        let mut out = vec![strings(&["lens", "reviews", "accepted_flaws", "reviews_with_flaws"])];
        for (lens, runs) in lenses {
            out.push(vec![lens.to_owned(), runs.len().to_string(), runs.iter().map(|run| run.flaws.unwrap_or(0)).sum::<i64>().to_string(), runs.iter().filter(|run| run.flaws.unwrap_or(0) > 0).count().to_string()]);
        }
        text += &format!("\n{}", table(&out));
    }
    if !splits.is_empty() {
        text += &format!("\n{}", split_summary(&splits.values().copied().collect::<Vec<_>>(), &rows));
    }
    text.push('\n');
    match closure(context, &rows) {
        Ok(closed) => text += &closed,
        Err(Failure::Refused(why)) => {
            let _ = writeln!(err, "NorthThreads: closure table needs GitHub: {why}");
        }
        Err(other) => return Err(other),
    }
    Ok(text)
}

pub(super) fn run(argv: &[String], context: &Context, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let result = Args::parse(argv, &["--since", "--group"], &[]).and_then(|args| summarise(&args, context, err));
    report("summary", result, out, err)
}
