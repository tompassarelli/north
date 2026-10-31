//! `threads planner-band`: each past item's measured difficulty and band, the cut points, and
//! how well each set of planning-time inputs predicts an item's band when it is left out.
use std::fmt::Write as _;
use std::path::Path;

use rusqlite::{Connection, OpenFlags};
use sha2::{Digest, Sha256};

use super::{BANDS, Band, Input, InputSet, Item, PREVIOUS, SHIPPED, cuts, items, predict, share, vote};
use crate::error::{NorthError, NorthResult};

const CANDIDATES: [InputSet; 7] = [
    PREVIOUS,
    InputSet { name: "previous + spec", inputs: &[Input::Category, Input::Scope, Input::Surface, Input::Eta, Input::PriorNotDone, Input::Spec] },
    InputSet { name: "previous + verify", inputs: &[Input::Category, Input::Scope, Input::Surface, Input::Eta, Input::PriorNotDone, Input::Verify] },
    SHIPPED,
    InputSet {
        name: "previous + spec verify repository",
        inputs: &[Input::Category, Input::Scope, Input::Surface, Input::Eta, Input::PriorNotDone, Input::Spec, Input::Verify, Input::Repository],
    },
    InputSet { name: "category eta", inputs: &[Input::Category, Input::Eta] },
    InputSet { name: "spec verify repository eta", inputs: &[Input::Spec, Input::Verify, Input::Repository, Input::Eta] },
];

/// How often one predictor named the band an item realized, overall and within each realized band.
struct Accuracy {
    name: &'static str,
    predictor: &'static str,
    matched: usize,
    hits: Vec<bool>,
    by_band: [(usize, usize); 3],
}

impl Accuracy {
    fn new(name: &'static str, predictor: &'static str) -> Self {
        Self { name, predictor, matched: 0, hits: Vec::new(), by_band: [(0, 0); 3] }
    }

    fn count(&mut self, predicted: Band, realized: Band) {
        let slot = &mut self.by_band[BANDS.iter().position(|band| *band == realized).unwrap_or(0)];
        slot.1 += 1;
        self.hits.push(predicted == realized);
        if predicted == realized {
            self.matched += 1;
            slot.0 += 1;
        }
    }
}

/// The exact two-sided McNemar p-value for paired hits: under no difference, each item only one
/// predictor matched is a fair coin flip.
pub(super) fn mcnemar(only_a: usize, only_b: usize) -> f64 {
    let trials = only_a + only_b;
    let fewer = only_a.min(only_b);
    let ln_choose = |k: usize| (1..=k).map(|i| ((trials - k + i) as f64 / i as f64).ln()).sum::<f64>();
    let tail: f64 = (0..=fewer).map(|k| (ln_choose(k) - trials as f64 * std::f64::consts::LN_2).exp()).sum();
    (2.0 * tail).min(1.0)
}

fn paired_line(shipped: &Accuracy, previous: &Accuracy) -> String {
    let only = |a: &Accuracy, b: &Accuracy| a.hits.iter().zip(&b.hits).filter(|(x, y)| **x && !**y).count();
    let (gained, lost) = (only(shipped, previous), only(previous, shipped));
    format!(
        "shipped ({}, {}) vs ed063ad7's rule ({}, {}): {gained} items only shipped matched, {lost} only the previous; exact McNemar p={:.4}; the shipped pair was picked as the best of these rows, so this p is optimistic",
        shipped.predictor,
        shipped.name,
        previous.predictor,
        previous.name,
        mcnemar(gained, lost)
    )
}

/// Each item predicted from all the others, with cut points measured on those others alone.
fn leave_one_out(items: &[Item]) -> Vec<Accuracy> {
    let mut majority = Accuracy::new("none", "always the most common band");
    let mut sets: Vec<Accuracy> = CANDIDATES.iter().map(|set| Accuracy::new(set.name, "mean difficulty")).collect();
    let mut votes: Vec<Accuracy> = CANDIDATES.iter().map(|set| Accuracy::new(set.name, "band vote")).collect();
    for (index, item) in items.iter().enumerate() {
        let training: Vec<Item> = items.iter().enumerate().filter(|(other, _)| *other != index).map(|(_, other)| other.clone()).collect();
        let cuts = cuts(&training);
        let realized = cuts.band(item.difficulty);
        let counts = BANDS.map(|band| training.iter().filter(|other| cuts.band(other.difficulty) == band).count());
        let common = BANDS[(0..3).max_by_key(|&slot| (counts[slot], usize::MAX - slot)).unwrap_or(0)];
        majority.count(common, realized);
        for ((set, mean), voted) in CANDIDATES.iter().zip(&mut sets).zip(&mut votes) {
            mean.count(cuts.band(predict(&item.facets, &training, set.inputs)), realized);
            voted.count(vote(&item.facets, &training, set.inputs, cuts), realized);
        }
    }
    std::iter::once(majority).chain(sets).chain(votes).collect()
}

const HELD_OUT_SEED: &str = "54";

/// The held-out check: a fifth of the items, chosen by a fixed-seed hash, ranked by the difficulty
/// the shipped inputs predict from the rest; off-rails items should crowd the top decile.
#[derive(Debug, PartialEq)]
pub(super) struct HeldOut {
    /// Each withheld item with the difficulty predicted for it, highest first.
    pub ranking: Vec<(String, f64)>,
    training: usize,
    withheld: usize,
    top_decile: usize,
    off_rails: usize,
    hits: usize,
    p: f64,
}

fn off_rails(item: &Item) -> bool {
    item.realized.not_done >= 2 || item.realized.escalations > 0 || item.realized.ratio.is_some_and(|ratio| ratio > 2.0)
}

/// The withheld fifth and the training rest, split by a hash of the seed and each item reference.
pub(super) fn held_out_split(items: &[Item]) -> (Vec<&Item>, Vec<&Item>) {
    let mut shuffled: Vec<(Vec<u8>, &Item)> = items.iter().map(|item| (Sha256::digest(format!("{HELD_OUT_SEED}:{}", item.item)).to_vec(), item)).collect();
    shuffled.sort_by(|a, b| a.0.cmp(&b.0));
    let mut withheld: Vec<&Item> = shuffled.into_iter().map(|(_, item)| item).collect();
    let training = withheld.split_off(items.len().div_ceil(5));
    (withheld, training)
}

pub(super) fn held_out(items: &[Item]) -> HeldOut {
    let (withheld_items, training) = held_out_split(items);
    let withheld = withheld_items.len();
    let mut ranked: Vec<(f64, &Item)> = withheld_items.iter().map(|item| (predict(&item.facets, training.iter().copied(), SHIPPED.inputs), *item)).collect();
    ranked.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.item.cmp(&b.1.item)));
    let top_decile = withheld / 10;
    let trials = ranked.iter().filter(|(_, item)| off_rails(item)).count();
    let hits = ranked[..top_decile].iter().filter(|(_, item)| off_rails(item)).count();
    let ln_choose = |k: usize| (1..=k).map(|i| ((trials - k + i) as f64 / i as f64).ln()).sum::<f64>();
    let p = (hits..=trials).map(|k| (ln_choose(k) + k as f64 * 0.1f64.ln() + (trials - k) as f64 * 0.9f64.ln()).exp()).sum::<f64>().min(1.0);
    let ranking = ranked.iter().map(|(score, item)| (item.item.clone(), *score)).collect();
    HeldOut { ranking, training: training.len(), withheld, top_decile, off_rails: trials, hits, p }
}

pub(super) fn held_out_line(check: &HeldOut) -> String {
    let rate = if check.off_rails == 0 { 0.0 } else { check.hits as f64 / check.off_rails as f64 };
    let passed = check.off_rails > 0 && rate > 0.1 && check.p < 0.05;
    format!(
        "held-out seed={HELD_OUT_SEED} training={} items={} top_decile={} off_rails={} hits={} rate={:.2}% chance=10% binomial_p={:.8} alpha=0.05 {}",
        check.training,
        check.withheld,
        check.top_decile,
        check.off_rails,
        check.hits,
        100.0 * rate,
        check.p,
        if passed { "PASS" } else { "FAIL" }
    )
}

fn table(rows: &[Vec<String>]) -> String {
    let widths: Vec<usize> = (0..rows[0].len()).map(|column| rows.iter().map(|row| row[column].chars().count()).max().unwrap_or(0)).collect();
    let line = |row: &Vec<String>| row.iter().zip(&widths).map(|(cell, width)| format!("{cell:<width$}")).collect::<Vec<_>>().join("  ").trim_end().to_owned();
    rows.iter().map(|row| format!("{}\n", line(row))).collect()
}

fn item_rows(items: &[Item], band: impl Fn(f64) -> Band) -> Vec<Vec<String>> {
    let header = ["item", "runs", "wall_min", "ETA", "missing_ETA", "ratio", "Not_done", "Stop", "restaffs", "escalations", "compactions", "difficulty", "band"];
    let mut rows = vec![header.map(str::to_owned).to_vec()];
    for item in items {
        let realized = &item.realized;
        rows.push(vec![
            item.item.clone(),
            realized.runs.to_string(),
            format!("{:.1}", realized.wall_min),
            format!("{}", realized.eta_min),
            realized.missing_eta.to_string(),
            realized.ratio.map_or_else(|| "-".to_owned(), |ratio| format!("{ratio:.3}")),
            realized.not_done.to_string(),
            realized.stops.to_string(),
            realized.restaffs.to_string(),
            realized.escalations.to_string(),
            realized.compactions.to_string(),
            format!("{:.6}", item.difficulty),
            band(item.difficulty).name().to_owned(),
        ]);
    }
    rows
}

fn accuracy_rows(accuracies: &[Accuracy], total: usize) -> Vec<Vec<String>> {
    let mut rows = vec![["predictor", "inputs", "matched", "share", "high", "xhigh", "max"].map(str::to_owned).to_vec()];
    for accuracy in accuracies {
        let mut row = vec![accuracy.predictor.to_owned(), accuracy.name.to_owned(), format!("{}/{total}", accuracy.matched), format!("{:.2}%", share(accuracy.matched, total))];
        row.extend(accuracy.by_band.iter().map(|(hit, of)| format!("{hit}/{of}")));
        rows.push(row);
    }
    rows
}

/// The planner-band readout over the threads ledger at `threads_db`, which it only reads.
pub fn band_readout(threads_db: &Path) -> NorthResult<String> {
    let refused = |error: rusqlite::Error| NorthError::Configuration(format!("threads ledger {}: {error}", threads_db.display()));
    let db = Connection::open_with_flags(threads_db, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX).map_err(refused)?;
    let (runs, no_item): (i64, i64) = db.query_row("SELECT count(*), count(*) - count(item) FROM runs", [], |row| Ok((row.get(0)?, row.get(1)?))).map_err(refused)?;
    let mut past = items(&db).map_err(refused)?;
    if past.is_empty() {
        return Err(NorthError::Usage("planner-band needs past items".into()));
    }
    past.sort_by(super::by_difficulty);
    let cuts = cuts(&past);
    let mut out = String::new();
    let _ = writeln!(out, "backfill runs={runs} items={} assigned_runs={}", past.len(), runs - no_item);
    let _ = writeln!(out, "no Item runs={no_item}");
    let _ = writeln!(out, "difficulty=sum(log1p(wall/ETA, Not done, restaffs, escalations, compactions)); wall=first start to last end; ratio omitted unless all runs have positive ETA; items with a run still going are left out");
    out.push_str(&table(&item_rows(&past, |difficulty| cuts.band(difficulty))));
    let _ = writeln!(out, "cut rule=minimum item classification errors; benefit classes=0/1/2+ signals (2+ reasoning/decision Stops, escalation, wall/ETA>2); ties prefer lower effort");
    let _ = writeln!(out, "cut points high=[0,{0:.6}) xhigh=[{0:.6},{1:.6}) max=[{1:.6},inf)", cuts.xhigh, cuts.max);
    let counts = BANDS.map(|band| past.iter().filter(|item| cuts.band(item.difficulty) == band).count());
    let shares = counts.map(|count| share(count, past.len()));
    let _ = writeln!(out, "measured high/xhigh/max={:.2}/{:.2}/{:.2}% items={}/{}/{} prior=90/8/2%", shares[0], shares[1], shares[2], counts[0], counts[1], counts[2]);
    let _ = writeln!(out, "held-out ranking=sha256 of seed and item picks a fifth; withheld items ranked by mean difficulty of their nearest training items over \"{}\"", SHIPPED.name);
    let _ = writeln!(out, "{}", held_out_line(&held_out(&past)));
    let _ = writeln!(out, "leave-one-out: each item's band predicted from the other {} with cut points measured on them; recommend uses band vote over \"{}\"", past.len() - 1, SHIPPED.name);
    let accuracies = leave_one_out(&past);
    out.push_str(&table(&accuracy_rows(&accuracies, past.len())));
    let find = |predictor: &str, name: &str| accuracies.iter().find(|accuracy| accuracy.predictor == predictor && accuracy.name == name);
    if let (Some(shipped), Some(previous)) = (find("band vote", SHIPPED.name), find("mean difficulty", PREVIOUS.name)) {
        let _ = writeln!(out, "{}", paired_line(shipped, previous));
    }
    Ok(out)
}
