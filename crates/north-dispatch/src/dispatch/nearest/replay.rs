//! `north dispatch route-replay [--days N] [--db PATH]`: walks the last N days of first-assignment worker
//! boxes in launch order and, using only runs that finished before each launch, compares the rung the old
//! routing started it on with the rung its nearest finished boxes pick. Both are projected through the same
//! escalation ladder, each rung's keep chance and cost read from the box's next K nearest boxes, disjoint from the K
//! that picked, so the ratio of usage per finished box compares rules, not luck or the pick's own evidence. Boxes whose tier was drawn by the Sonnet and Opus arms are
//! also scored by the replay method: where the new pick matches the drawn arm, the logged outcome stands.
use super::cause::{Cause, cause};
use super::pick::{FLOOR, Finished, Fit, Index, K, Rung, fits, pick};
use super::shape::{Reach, Shape};
use north_threads::threads::calibration::{Knobs, Raw, escalated, raw_runs, recommend, tier_key};
use rusqlite::{Connection, OpenFlags};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

const USAGE: &str = "usage: north dispatch route-replay [--days N] [--db PATH]";
const ATTEMPTS: usize = 3;
const RERUN_HOURS: f64 = 24.0;

struct Box {
    run: usize,
    item: String,
    started: f64,
}

/// One run with what the replay reads of it.
struct Ran {
    raw: Raw,
    started: f64,
    ended: f64,
    rung: Option<Rung>,
    arm: Option<String>,
    query: Shape,
    finished: Option<Finished>,
}

pub fn run(arguments: &[String]) -> u8 {
    let mut days = 14.0;
    let mut db = PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state/threads/snapshot.db");
    let mut words = arguments.iter();
    while let Some(word) = words.next() {
        match (word.as_str(), words.next()) {
            ("--days", Some(value)) if value.parse::<f64>().is_ok() => days = value.parse().unwrap_or(days),
            ("--db", Some(value)) => db = PathBuf::from(value),
            _ => {
                eprintln!("{USAGE}");
                return 2;
            }
        }
    }
    match replay(&db, days) {
        Ok(lines) => {
            lines.iter().for_each(|line| println!("{line}"));
            0
        }
        Err(why) => {
            eprintln!("north dispatch route-replay: {why}");
            1
        }
    }
}

fn seconds(stamp: &str) -> f64 {
    north_base::foundation::clock::Timestamp::parse_loose(stamp).map_or(f64::NAN, |at| at.unix_seconds() as f64)
}

fn read(db: &std::path::Path) -> Result<Vec<Ran>, String> {
    let connection = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|error| format!("{}: {error}", db.display()))?;
    let failed = |error: rusqlite::Error| error.to_string();
    let followed = escalated(&connection).map_err(failed)?;
    let raws = raw_runs(&connection, "outcome NOT IN ('running','idle') AND ended IS NOT NULL", &[]).map_err(failed)?;
    let mut statement = connection.prepare("SELECT agent, report, coalesce(brief_text, ''), arm FROM runs").map_err(failed)?;
    let texts: BTreeMap<String, (String, String, Option<String>)> = statement.query_map([], |row| Ok((row.get(0)?, (row.get(1)?, row.get(2)?, row.get(3)?)))).map_err(failed)?.collect::<Result<_, _>>().map_err(failed)?;
    let items: Vec<String> = raws.iter().filter_map(|raw| raw.item.clone()).filter(|item| item.contains('#')).collect::<BTreeSet<_>>().into_iter().collect();
    let footprints = north_graph_ground::footprints(&north_graph_ground::graph_path(), &items).unwrap_or_default();
    Ok(raws
        .into_iter()
        .map(|raw| {
            let (report, brief, arm) = texts.get(&raw.agent).cloned().unwrap_or_default();
            let item = raw.item.clone().unwrap_or_default();
            let footprint = footprints.get(&item);
            let text = if brief.is_empty() { footprint.map(|found| format!("{}\n{}", found.title, found.boxes.join("\n"))).unwrap_or_default() } else { brief };
            let repo = item.split_once('#').map(|(repo, _)| repo.rsplit('/').next().unwrap_or(repo));
            let query = Shape::read(&text, repo);
            let rung = Rung::of_key(&tier_key(&raw.tier));
            let succeeded = raw.succeeded(followed.contains(&raw.agent));
            let finished = rung.zip(cause(&raw.outcome, succeeded, &report)).map(|(rung, cause)| Finished { shape: footprint.map_or_else(|| query.clone(), |found| query.clone().touched(&found.touched)), rung, cause, usd: raw.api_usd });
            let ended = seconds(&raw.ended);
            let started = raw.started.as_deref().map_or(ended - raw.minutes.unwrap_or(0.0) * 60.0, seconds);
            Ran { raw, started, ended, rung, arm: arm.filter(|arm| !arm.is_empty()), query, finished }
        })
        .filter(|ran| ran.started.is_finite() && ran.ended.is_finite())
        .collect())
}

/// First assignments in the window: a run that follows no other and starts more than a day after the last
/// run on its item ended.
fn boxes(runs: &[Ran], since: f64) -> Vec<Box> {
    let mut by_item: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (index, ran) in runs.iter().enumerate() {
        if let Some(item) = ran.raw.item.as_deref().filter(|item| !item.is_empty()) {
            by_item.entry(item).or_default().push(index);
        }
    }
    let mut found = Vec::new();
    for (item, mut indices) in by_item {
        indices.sort_by(|a, b| runs[*a].started.total_cmp(&runs[*b].started));
        let mut last_end = f64::NEG_INFINITY;
        for index in indices {
            let ran = &runs[index];
            if ran.raw.follows.is_none() && ran.started - last_end > RERUN_HOURS * 3600.0 && ran.started >= since && ran.rung.is_some() {
                found.push(Box { run: index, item: item.to_owned(), started: ran.started });
            }
            last_end = last_end.max(ran.ended);
        }
    }
    found.sort_by(|a, b| a.started.total_cmp(&b.started));
    found
}

/// Expected API-equivalent dollars and kept boxes of one box started on `start` and escalated one rung on
/// each worker-caused failure, a harness failure repeating its rung.
fn chain(start: Rung, keep: &dyn Fn(Rung) -> f64, cost: &dyn Fn(Rung) -> f64, harness: f64) -> (f64, f64) {
    let (mut usd, mut kept, mut reach, mut rung) = (0.0, 0.0, 1.0, Some(start));
    for _ in 0..ATTEMPTS {
        let Some(on) = rung else { break };
        usd += reach * cost(on) / (1.0 - harness);
        kept += reach * keep(on);
        reach *= 1.0 - keep(on);
        rung = on.up();
    }
    (usd, kept)
}

#[derive(Default)]
struct Tally {
    usd: f64,
    kept: f64,
    first: f64,
    boxes: usize,
}

impl Tally {
    fn add(&mut self, (usd, kept): (f64, f64), first: f64) {
        self.usd += usd;
        self.kept += kept;
        self.first += first;
        self.boxes += 1;
    }

    fn per_kept(&self) -> f64 {
        self.usd / self.kept.max(f64::MIN_POSITIVE)
    }

    fn line(&self, name: &str) -> String {
        format!("{name}: {} boxes, {:.1} finished, ${:.2} per finished box, first-try {:.0}%", self.boxes, self.kept, self.per_kept(), 100.0 * self.first / self.boxes.max(1) as f64)
    }
}

fn median(values: &mut [f64]) -> Option<f64> {
    values.sort_by(f64::total_cmp);
    values.get(values.len() / 2).copied()
}

/// What every rung did over `past`: worker-caused keep share, median dollars per run and harness share.
struct Pooled {
    keep: BTreeMap<Rung, f64>,
    usd: BTreeMap<Rung, f64>,
    harness: f64,
}

fn pooled(past: &[&Finished]) -> Pooled {
    let mut tally: BTreeMap<Rung, (f64, f64, Vec<f64>)> = BTreeMap::new();
    let mut harness = 0.0;
    for run in past {
        let entry = tally.entry(run.rung).or_default();
        entry.2.extend(run.usd);
        match run.cause {
            Cause::Kept => {
                entry.0 += 1.0;
                entry.1 += 1.0;
            }
            Cause::Worker(_) => entry.1 += 1.0,
            Cause::Harness(_) => harness += 1.0,
        }
    }
    let keep = tally.iter().map(|(rung, (kept, judged, _))| (*rung, kept / judged.max(1.0))).collect();
    let usd = tally.iter_mut().filter_map(|(rung, (_, _, costs))| Some((*rung, median(costs)?))).collect();
    Pooled { keep, usd, harness: harness / past.len().max(1) as f64 }
}

fn replay(db: &std::path::Path, days: f64) -> Result<Vec<String>, String> {
    let runs = read(db)?;
    let now = runs.iter().map(|ran| ran.ended).fold(f64::NEG_INFINITY, f64::max);
    let since = now - days * 86_400.0;
    let index = Index::build(&runs.iter().map(|ran| &ran.query).collect::<Vec<_>>());
    let knobs = Knobs::registry();
    let mut order: Vec<usize> = (0..runs.len()).collect();
    order.sort_by(|a, b| runs[*a].ended.total_cmp(&runs[*b].ended));
    let (mut old, mut ran_on, mut new, mut observed, mut drawn, mut matched) = (Tally::default(), Tally::default(), Tally::default(), Tally::default(), Tally::default(), Tally::default());
    let (mut moved, mut capped, mut fell_back, mut localized, mut alone) = (BTreeMap::<(Rung, Rung), usize>::new(), 0, 0, 0, 0);
    let mut reaches: BTreeMap<Reach, usize> = BTreeMap::new();
    let causes = causes(&runs, since);
    let boxes = boxes(&runs, since);
    for found in &boxes {
        let ran = &runs[found.run];
        let past: Vec<usize> = order.iter().copied().take_while(|other| runs[*other].ended < found.started).filter(|other| runs[*other].finished.is_some()).collect();
        let finished: Vec<&Finished> = past.iter().filter_map(|other| runs[*other].finished.as_ref()).collect();
        let pool = pooled(&finished);
        let mut neighbours = nearest(&runs, &index, &past, found.run);
        let scoring = neighbours.split_off(neighbours.len().min(K));
        alone += usize::from(neighbours.is_empty());
        let fit = fits(&neighbours);
        let scored = fits(&scoring);
        let logged = ran.rung.expect("boxes carry a rung");
        let picked = pick(&ran.query, &fit, &pool.keep);
        let cell = facet_cell(&knobs, &runs, &past, ran).unwrap_or(logged);
        localized += usize::from(ran.query.localized());
        *reaches.entry(ran.query.reach()).or_default() += 1;
        let rung = match &picked {
            Some(pick) => {
                capped += usize::from(pick.capped);
                pick.rung
            }
            None => {
                fell_back += 1;
                cell
            }
        };
        *moved.entry((logged, rung)).or_default() += 1;
        let keep = |rung: Rung| scored.get(&rung).filter(|fit| fit.judged_runs > 0).map_or_else(|| pool.keep.get(&rung).copied().unwrap_or(0.5), |fit: &Fit| fit.fitness(pool.keep.get(&rung).copied().unwrap_or(0.5)));
        let cost = |rung: Rung| scored.get(&rung).filter(|fit| fit.runs >= 3).and_then(Fit::median_usd).or_else(|| pool.usd.get(&rung).copied()).unwrap_or(0.0);
        let judged: f64 = scored.values().map(|fit| fit.judged + fit.harness).sum();
        let harness = if judged > 0.0 { scored.values().map(|fit| fit.harness).sum::<f64>() / judged } else { pool.harness }.min(0.8);
        old.add(chain(cell, &keep, &cost, harness), keep(cell));
        ran_on.add(chain(logged, &keep, &cost, harness), keep(logged));
        new.add(chain(rung, &keep, &cost, harness), keep(rung));
        let realized = realized(&runs, found);
        observed.add(realized, f64::from(u8::from(matches!(ran.finished.as_ref().map(|finished| finished.cause), Some(Cause::Kept)))));
        if matches!(ran.arm.as_deref(), Some("sonnet-high" | "worker")) {
            drawn.add(realized, f64::from(u8::from(matches!(ran.finished.as_ref().map(|finished| finished.cause), Some(Cause::Kept)))));
            if rung == logged {
                matched.add(realized, f64::from(u8::from(matches!(ran.finished.as_ref().map(|finished| finished.cause), Some(Cause::Kept)))));
            }
        }
    }
    Ok(report(days, &boxes, &causes, &reaches, &moved, (localized, capped, fell_back, alone), [&observed, &ran_on, &old, &new, &drawn, &matched]))
}

/// Each run that ended in the window, by the cause of its outcome.
fn causes(runs: &[Ran], since: f64) -> BTreeMap<String, (usize, f64)> {
    let mut causes: BTreeMap<String, (usize, f64)> = BTreeMap::new();
    for ran in runs.iter().filter(|ran| ran.ended >= since) {
        let name = match ran.finished.as_ref().map(|finished| finished.cause) {
            Some(Cause::Kept) => "kept".to_owned(),
            Some(Cause::Worker(why)) => format!("worker {why}"),
            Some(Cause::Harness(why)) => format!("harness {why}"),
            None => "unattributed".to_owned(),
        };
        let entry = causes.entry(name).or_default();
        entry.0 += 1;
        entry.1 += ran.raw.api_usd.unwrap_or(0.0);
    }
    causes
}

/// The `2K` finished runs in `past` most like run `query`: the first `K` pick the rung and the next `K` score it, so the
/// pick is never judged on the evidence that chose it, at similarity `FLOOR` or more, most alike first.
fn nearest<'a>(runs: &'a [Ran], index: &Index, past: &[usize], query: usize) -> Vec<(f64, &'a Finished)> {
    let item = runs[query].raw.item.as_deref();
    let mut near: Vec<(f64, &Finished)> = past.iter().filter(|other| runs[**other].raw.item.as_deref() != item).filter_map(|other| runs[*other].finished.as_ref().map(|finished| (index.similarity((query, &runs[query].query), (*other, &finished.shape)), finished))).filter(|(similarity, _)| *similarity >= FLOOR).collect();
    near.sort_by(|a, b| b.0.total_cmp(&a.0));
    near.truncate(2 * K);
    near
}

/// The facet cell's pick over the runs finished before the box, as today's routing reads the ledger.
fn facet_cell(knobs: &Knobs, runs: &[Ran], past: &[usize], ran: &Ran) -> Option<Rung> {
    let followed: Vec<_> = past.iter().map(|other| &runs[*other]).filter(|other| other.raw.follows.is_none()).map(|other| other.raw.recommend_run(matches!(other.finished.as_ref().map(|finished| finished.cause), Some(Cause::Kept)))).collect();
    let asked = ran.raw.cells.clone().map(|value| (value != "-").then_some(value));
    recommend(knobs, &followed, &asked, &BTreeMap::new(), true).tier.as_deref().and_then(Rung::of_key)
}

/// What the box really cost and whether it finished: every run on its item from its launch until the next
/// first assignment of that item.
fn realized(runs: &[Ran], found: &Box) -> (f64, f64) {
    let chained: Vec<&Ran> = runs.iter().filter(|other| other.raw.item.as_deref() == Some(found.item.as_str()) && other.started >= found.started && other.started - found.started < 3.0 * RERUN_HOURS * 3600.0).collect();
    let usd = chained.iter().map(|other| other.raw.api_usd.unwrap_or(0.0)).sum();
    let kept = chained.iter().any(|other| matches!(other.finished.as_ref().map(|finished| finished.cause), Some(Cause::Kept)));
    (usd, f64::from(u8::from(kept)))
}

fn report(days: f64, boxes: &[Box], causes: &BTreeMap<String, (usize, f64)>, reaches: &BTreeMap<Reach, usize>, moved: &BTreeMap<(Rung, Rung), usize>, (localized, capped, fell_back, alone): (usize, usize, usize, usize), [observed, ran_on, old, new, drawn, matched]: [&Tally; 6]) -> Vec<String> {
    let mut lines = vec![format!("route replay over the last {days} days: {} first-assignment boxes", boxes.len())];
    let (total, spent): (usize, f64) = causes.values().fold((0, 0.0), |(runs, usd), (count, cost)| (runs + count, usd + cost));
    lines.push(format!("outcome causes of {total} runs ended in the window, ${spent:.0} API-equivalent:"));
    let mut sorted: Vec<_> = causes.iter().collect();
    sorted.sort_by(|a, b| b.1.1.total_cmp(&a.1.1));
    lines.extend(sorted.iter().map(|(name, (count, usd))| format!("  {name}: {count} runs, ${usd:.0} ({:.0}%)", 100.0 * usd / spent.max(f64::MIN_POSITIVE))));
    lines.push(format!("reach of the boxes' named files: {}", reaches.iter().map(|(reach, count)| format!("{reach:?} {count}")).collect::<Vec<_>>().join(", ")));
    lines.push(format!("{localized} boxes name their files and symbols; {capped} picks capped at sonnet; {fell_back} boxes fell back to the facet cell, {alone} of them with no finished box at similarity {FLOOR} or more"));
    lines.push("old rung -> new rung:".to_owned());
    lines.extend(moved.iter().map(|((from, to), count)| format!("  {} -> {}: {count}", from.name(), to.name())));
    lines.push(observed.line("observed (old rule, as run)"));
    lines.push(ran_on.line("projected rungs as run (facet cell, arm draws and named tiers)"));
    lines.push(old.line("projected old rule (facet cell for every box)"));
    lines.push(new.line("projected new rule"));
    lines.push(format!("projected gain in finished boxes per dollar: {:.2}x", old.per_kept() / new.per_kept().max(f64::MIN_POSITIVE)));
    lines.push(drawn.line("arm-drawn boxes, as run"));
    lines.push(matched.line("arm-drawn boxes where the new pick matches the draw"));
    lines
}
