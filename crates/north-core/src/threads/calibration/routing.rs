//! `north threads routing-compare`: the campaign arms by category, read-only from the ledger.
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::sync::LazyLock;

use regex::Regex;
use rusqlite::Row;

use super::boxes::moment;
use super::{cell_value, median, provider, quantile, tier_rank};
use crate::foundation::clock::Timestamp;
use crate::platform::pystats::float_sum;
use crate::platform::pystats::wilson;
use crate::platform::twister::Twister;
use crate::threads::verb::{Context, Failure, Outcome, open, report, table};

const CAMPAIGN_START: &str = "2026-10-12T12:27:27Z";
const BASELINE_START: &str = "2026-10-05T12:27:27Z";
const SPLIT_START: &str = "2026-10-15T02:00Z";
const SPLIT_END: &str = "2026-10-15T16:00Z";
const ARMS: [(&str, &str, &str); 3] = [("baseline", BASELINE_START, CAMPAIGN_START), ("codex-only", CAMPAIGN_START, SPLIT_START), ("split-after", SPLIT_START, SPLIT_END)];
const RESAMPLES: usize = 2000;
const MIN_OBSERVATIONS: usize = 5;

static REWORK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)(?:^|\n)\s*(?:Rework(?:ed)?|Revert(?:ed)?):").expect("rework pattern"));

struct Run {
    agent: String,
    role: String,
    outcome: String,
    item: Option<String>,
    follows: Option<String>,
    tier: String,
    category: Option<String>,
    ended: String,
    staffed: Option<String>,
    started: Option<String>,
    landed: Option<i64>,
    landed_at: Option<String>,
    repeats: Option<i64>,
    report: String,
    arm: Option<String>,
    account: Option<String>,
}

impl Run {
    fn read(row: &Row) -> rusqlite::Result<Self> {
        Ok(Self {
            agent: row.get("agent")?,
            role: row.get("role")?,
            outcome: row.get("outcome")?,
            item: row.get("item")?,
            follows: row.get("follows")?,
            tier: row.get("tier")?,
            category: row.get("category")?,
            ended: row.get("ended")?,
            staffed: row.get("staffed")?,
            started: row.get("started")?,
            landed: row.get("landed")?,
            landed_at: row.get("landed_at")?,
            repeats: row.get("repeats")?,
            report: row.get("report")?,
            arm: row.get("arm")?,
            account: row.get("account")?,
        })
    }

    fn landing(&self) -> Option<Timestamp> {
        moment(present(&self.landed_at).unwrap_or(&self.ended))
    }

    fn begin(&self) -> Option<Timestamp> {
        moment(present(&self.staffed).or_else(|| present(&self.started)).unwrap_or(&self.ended))
    }
}

fn present(text: &Option<String>) -> Option<&str> {
    text.as_deref().filter(|text| !text.is_empty())
}

struct Box<'a> {
    category: String,
    runs: Vec<&'a Run>,
    landed: Vec<&'a Run>,
    wall: Option<f64>,
    first_try: Option<bool>,
    adverse: Option<bool>,
}

fn quality(runs: &[&Run]) -> (Option<bool>, Option<bool>) {
    let by_agent: BTreeMap<&str, &Run> = runs.iter().map(|run| (run.agent.as_str(), *run)).collect();
    let followed = |run: &Run| run.follows.as_deref().and_then(|follows| by_agent.get(follows));
    let rework = runs.iter().any(|run| run.repeats.unwrap_or(0) > 0 || followed(run).is_some() || REWORK.is_match(&run.report));
    let escalated = runs.iter().any(|run| run.outcome == "escalated" || run.arm.as_deref() == Some("escalate") || followed(run).is_some_and(|earlier| tier_rank(&run.tier) > tier_rank(&earlier.tier)));
    let not_done = runs.iter().any(|run| run.outcome == "not done");
    let unknown = runs.iter().any(|run| run.outcome != "done");
    let first_try = if rework || escalated || not_done { Some(false) } else if unknown { None } else { Some(true) };
    let adverse = if rework || not_done { Some(true) } else if unknown { None } else { Some(false) };
    (first_try, adverse)
}

fn wall(runs: &[&Run], landed: &[&Run], finish: Timestamp) -> Option<f64> {
    let staffed: Vec<Timestamp> = runs.iter().filter_map(|run| present(&run.staffed).and_then(moment)).collect();
    if staffed.len() != runs.len() || landed.iter().any(|run| present(&run.landed_at).is_none()) {
        return None;
    }
    staffed.iter().min().map(|first| (finish.unix_nanos() - first.unix_nanos()) as f64 / 60e9)
}

fn boxes<'a>(rows: &'a [Run], start: Timestamp, end: Timestamp) -> (Vec<Box<'a>>, Vec<&'a Run>) {
    let workers: Vec<&Run> = rows.iter().filter(|run| run.role == "worker" && run.outcome != "running").collect();
    let mut by_item: BTreeMap<&str, Vec<&Run>> = BTreeMap::new();
    for run in &workers {
        if let Some(item) = present(&run.item) {
            by_item.entry(item).or_default().push(run);
        }
    }
    let within = |run: &Run| run.landing().is_some_and(|landing| start <= landing && landing < end);
    let mut found = Vec::new();
    for history in by_item.into_values() {
        let landed: Vec<&Run> = history.iter().copied().filter(|run| run.landed == Some(1) && within(run)).collect();
        let Some(finish) = landed.iter().filter_map(|run| run.landing()).max() else { continue };
        let mut runs: Vec<&Run> = history.iter().copied().filter(|run| landed.iter().any(|kept| kept.agent == run.agent) || moment(&run.ended).is_some_and(|ended| ended <= finish)).collect();
        runs.sort_by(|left, right| left.begin().cmp(&right.begin()).then_with(|| left.agent.cmp(&right.agent)));
        let (first_try, adverse) = quality(&runs);
        let wall = wall(&runs, &landed, finish);
        found.push(Box {
            category: cell_value("category", runs[0].category.as_deref()),
            first_try,
            adverse,
            runs,
            landed,
            wall,
        });
    }
    let no_item = workers.into_iter().filter(|run| present(&run.item).is_none() && within(run)).collect();
    (found, no_item)
}

fn bootstrap(values: &[f64], statistic: fn(&[f64]) -> f64) -> String {
    if values.len() < MIN_OBSERVATIONS {
        return format!("n/a (n={})", values.len());
    }
    let mut twister = Twister::seeded(0);
    let mut samples: Vec<f64> = (0..RESAMPLES).map(|_| statistic(&twister.choices(values, values.len()))).collect();
    samples.sort_by(f64::total_cmp);
    format!("{:.2} [{:.2},{:.2}] (n={})", statistic(values), quantile(&samples, 0.025).unwrap_or(f64::NAN), quantile(&samples, 0.975).unwrap_or(f64::NAN), values.len())
}

fn rate(count: usize, n: usize) -> String {
    if n < MIN_OBSERVATIONS {
        return format!("n/a (n={n})");
    }
    let (low, high) = wilson(count as f64 / n as f64, n);
    format!("{:.2}% [{:.2},{:.2}] ({count}/{n})", 100.0 * count as f64 / n as f64, 100.0 * low, 100.0 * high)
}

fn middle(values: &[f64]) -> f64 {
    median(values).unwrap_or(f64::NAN)
}

fn mean(values: &[f64]) -> f64 {
    float_sum(values.iter().copied()) / values.len() as f64
}

fn costs(db: &rusqlite::Connection) -> rusqlite::Result<Costs> {
    let present: bool = db.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'run_costs'", [], |row| row.get::<_, i64>(0).map(|count| count > 0))?;
    let mut costs: BTreeMap<String, BTreeMap<String, f64>> = BTreeMap::new();
    if present {
        let mut statement = db.prepare("SELECT agent, account, SUM(cost_usd) FROM run_costs GROUP BY agent, account")?;
        for line in statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, f64>(2)?)))? {
            let (agent, account, usd) = line?;
            costs.entry(agent).or_default().insert(account, usd);
        }
    }
    Ok(costs)
}

type Costs = BTreeMap<String, BTreeMap<String, f64>>;

const LEGEND: &str = "source reviews=unavailable from retired peer files; multi-arm items require landing SHA prefix\n\
    boxes=distinct worker items with landed=1 in [start,end); landing time=landed_at, else ended; category=first worker run\n\
    history=landed runs and worker runs ended by last landing; wall requires every staffed and landing timestamp\n\
    first_try=no Not done/rework/escalation before landing; adverse=Not done or repeats/Follows/rework/revert report marker; escalation=outcome/arm/tier increase\n\
    quality=independent metrics; decisive failure/adverse evidence wins; otherwise any outcome other than done is unknown; rates use known observations\n\
    mix=worker run counts by provider(tier)/tier; review minutes=observed total; points require known accounts and every run's allocation, never imputed\n\
    CI=95%; bootstrap=2000 resamples, seed=0, percentile; rates=Wilson; statistics require n>=5\n";

fn quality_coverage(name: &str, category: &str, bs: &[&Box], first_try: usize, adverse: usize) -> String {
    let total = bs.len();
    format!(
        "quality coverage {name} {category} first_try known={first_try}/{total} unknown={}/{total} adverse known={adverse}/{total} unknown={}/{total}\n",
        total - first_try,
        total - adverse
    )
}

fn category_row(name: &str, category: &str, bs: &[&Box]) -> (String, Vec<String>) {
    let mut mix: BTreeMap<String, usize> = BTreeMap::new();
    for run in bs.iter().flat_map(|each| each.runs.iter()) {
        *mix.entry(format!("{}/{}", provider(&run.tier), run.tier)).or_default() += 1;
    }
    let first_try: Vec<bool> = bs.iter().filter_map(|each| each.first_try).collect();
    let adverse: Vec<bool> = bs.iter().filter_map(|each| each.adverse).collect();
    let mixed = mix.iter().map(|(key, count)| format!("{key}:{count}")).collect::<Vec<_>>().join(";");
    let walls: Vec<f64> = bs.iter().filter_map(|each| each.wall).collect();
    let row = vec![
        name.to_owned(),
        category.to_owned(),
        bs.len().to_string(),
        if mixed.is_empty() { "-".to_owned() } else { mixed },
        bootstrap(&walls, middle),
        rate(first_try.iter().filter(|won| **won).count(), first_try.len()),
        rate(adverse.iter().filter(|won| **won).count(), adverse.len()),
        "n/a (n=0)".to_owned(),
        format!("0/{}", bs.len()),
    ];
    (quality_coverage(name, category, bs, first_try.len(), adverse.len()), row)
}

fn account_row(name: &str, category: &str, account: &str, bs: &[&Box], costs: &Costs, prices: &BTreeMap<String, f64>) -> Vec<String> {
    let spent = |run: &Run| costs.get(&run.agent).and_then(|by| by.get(account)).copied();
    let own = |run: &Run| present(&run.account).is_some_and(|own| costs.get(&run.agent).is_some_and(|by| by.contains_key(own)));
    let usd: Vec<f64> = bs.iter().map(|each| float_sum(each.runs.iter().map(|run| spent(run).unwrap_or(0.0)))).collect();
    let covered = bs.iter().filter(|each| each.runs.iter().any(|run| spent(run).is_some())).count();
    let complete = bs.iter().filter(|each| each.runs.iter().all(|run| own(run))).count();
    let price = prices.get(account).copied().filter(|price| *price > 0.0);
    let points: Vec<f64> = match price {
        Some(price) if complete == bs.len() => usd.iter().map(|value| value * 100.0 / price).collect(),
        _ => Vec::new(),
    };
    let interval = if points.is_empty() { format!("n/a (n={})", if price.is_some() { complete } else { 0 }) } else { bootstrap(&points, mean) };
    vec![name.to_owned(), category.to_owned(), account.to_owned(), bs.len().to_string(), covered.to_string(), interval]
}

fn accounts_of<'b>(bs: &[&'b Box], costs: &'b Costs) -> BTreeSet<&'b str> {
    bs.iter()
        .flat_map(|each| each.runs.iter())
        .flat_map(|run| present(&run.account).into_iter().chain(costs.get(&run.agent).into_iter().flat_map(|by| by.keys().map(String::as_str))))
        .collect()
}

struct Arm<'a> {
    name: &'static str,
    start: &'static str,
    end: &'static str,
    found: Vec<Box<'a>>,
    no_item: Vec<&'a Run>,
}

fn arm_section(arm: &Arm, costs: &Costs, prices: &BTreeMap<String, f64>, out: &mut Vec<Vec<String>>, point_out: &mut Vec<Vec<String>>) -> String {
    let (name, found) = (arm.name, &arm.found);
    let mut text = format!("window {name} [{},{})\n", arm.start, arm.end);
    text += &format!("no Item runs {name} n={} landed={}\n", arm.no_item.len(), arm.no_item.iter().filter(|run| run.landed == Some(1)).count());
    text += &format!(
        "coverage {name} category={}/{} landing_timestamp={}/{}\n",
        found.iter().filter(|each| each.category != "unknown").count(),
        found.len(),
        found.iter().filter(|each| each.landed.iter().all(|run| present(&run.landed_at).is_some())).count(),
        found.len()
    );
    text += &format!("review ambiguity {name} boxes=0/{}\n", found.len());
    let categories: BTreeSet<&str> = found.iter().map(|each| each.category.as_str()).collect();
    for category in categories.into_iter().chain(["overall"]) {
        let bs: Vec<&Box> = found.iter().filter(|each| category == "overall" || each.category == category).collect();
        let (coverage, row) = category_row(name, category, &bs);
        text += &coverage;
        out.push(row);
        for account in accounts_of(&bs, costs) {
            point_out.push(account_row(name, category, account, &bs, costs, prices));
        }
    }
    text
}

fn routing_compare(context: &Context) -> Outcome {
    let db = open(context.database, false)?;
    let rows: Vec<Run> = db.prepare("SELECT * FROM runs")?.query_map([], Run::read)?.collect::<Result<_, _>>()?;
    let prices = context.outside.prices().map_err(|why| Failure::Refused(format!("cannot read account costs {why}")))?;
    let costs = costs(&db)?;
    let parse = |text: &str| moment(text).ok_or_else(|| Failure::Refused(format!("{text} is not a time")));
    let mut arms = Vec::new();
    for (name, start, end) in ARMS {
        let (found, no_item) = boxes(&rows, parse(start)?, parse(end)?);
        arms.push(Arm { name, start, end, found, no_item });
    }
    let ledger = std::fs::canonicalize(context.database).unwrap_or_else(|_| context.database.to_path_buf());
    let mut text = format!("source ledger={} (read-only); campaign=south c2edcdf\n", ledger.display());
    text += &format!("source account prices={}; plan_usage.costs_by_id; points=run_costs USD*100/account usd_per_week\n", context.outside.costs_path().display());
    text += LEGEND;
    if parse(SPLIT_END)?.unix_nanos() > context.now.unix_nanos() {
        text += &format!("window open until {SPLIT_END}\n");
    }
    let mut out = vec![["arm", "category", "landed_boxes", "provider/tier_runs", "wall_min_median_CI", "first_try_CI", "adverse_CI", "review_min", "review_coverage"].map(str::to_owned).to_vec()];
    let mut point_out = vec![["arm", "category", "account", "landed_boxes", "allocated_boxes", "points_per_box_CI"].map(str::to_owned).to_vec()];
    for arm in &arms {
        text += &arm_section(arm, &costs, &prices, &mut out, &mut point_out);
    }
    text += &table(&out);
    text.push('\n');
    text += &table(&point_out);
    Ok(text)
}

pub(super) fn run(context: &Context, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    report("routing-compare", routing_compare(context), out, err)
}
