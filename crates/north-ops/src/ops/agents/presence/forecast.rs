use std::collections::{BTreeMap, HashMap};

use time::{Date, Duration, Time};

use super::{IdleRow, Moment, SLOT_MIN, WINDOW_DAYS, Zone};

fn unique_sorted(mut moments: Vec<Moment>) -> Vec<Moment> {
    moments.sort();
    moments.dedup();
    moments
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.len() % 2 == 1 { values[middle] } else { (values[middle - 1] + values[middle]) / 2.0 }
}

fn minute_of_day(at: Moment) -> i64 {
    i64::from(at.hour()) * 60 + i64::from(at.minute())
}

pub fn sleep_block(turns: &[Moment], now: Moment, zone: &Zone) -> Option<(f64, f64)> {
    let recent = unique_sorted(turns.iter().copied().filter(|at| now - Duration::days(14) <= *at && *at <= now).collect());
    let mut gaps: BTreeMap<Date, (f64, f64, f64)> = BTreeMap::new();
    for pair in recent.windows(2) {
        let duration = (pair[1] - pair[0]).as_seconds_f64() / 60.0;
        if !(120.0..=1440.0).contains(&duration) {
            continue;
        }
        let start = zone.localize(pair[0]);
        if gaps.get(&start.date()).is_none_or(|gap| duration > gap.0) {
            let minute = minute_of_day(start);
            let unwrapped = (if minute >= 720 { minute } else { minute + 1440 }) as f64;
            gaps.insert(start.date(), (duration, unwrapped, unwrapped + duration));
        }
    }
    if gaps.is_empty() {
        return None;
    }
    let starts = median(gaps.values().map(|gap| gap.1).collect());
    let ends = median(gaps.values().map(|gap| gap.2).collect());
    Some((starts.rem_euclid(1440.0), ends.rem_euclid(1440.0)))
}

pub struct Forecast<'a> {
    zone: &'a Zone,
    daily: HashMap<i64, (u32, u32)>,
    weekly: HashMap<(u8, i64), (u32, u32)>,
}

fn day_slot(local: Moment) -> i64 {
    minute_of_day(local) / *SLOT_MIN
}

impl<'a> Forecast<'a> {
    pub fn new(now: Moment, turns: &[Moment], idle: &[&IdleRow], zone: &'a Zone) -> Self {
        let mut forecast = Self { zone, daily: HashMap::new(), weekly: HashMap::new() };
        let step = Duration::minutes(*SLOT_MIN);
        let start = now - Duration::days(*WINDOW_DAYS);
        let within = |at: &Moment| start <= *at && *at <= now;
        let turns = unique_sorted(turns.iter().copied().filter(within).collect());
        let mut idle: Vec<(Moment, bool)> = idle.iter().filter(|row| within(&row.at)).filter_map(|row| Some((row.at, row.idle?))).collect();
        idle.sort();
        let first = match (turns.first(), idle.first()) {
            (Some(turn), Some(row)) => if row.0 < *turn { row.0 } else { *turn },
            (Some(turn), None) => *turn,
            (None, Some(row)) => row.0,
            (None, None) => return forecast,
        };
        let mut slot = first.replace_time(Time::MIDNIGHT);
        let (mut turn_index, mut idle_index, mut seat) = (0, 0, None);
        while slot < now {
            while turn_index < turns.len() && turns[turn_index] < slot - step {
                turn_index += 1;
            }
            while idle_index < idle.len() && idle[idle_index].0 <= slot {
                seat = Some(!idle[idle_index].1);
                idle_index += 1;
            }
            let active = seat == Some(true) || turns.get(turn_index).is_some_and(|turn| *turn <= now.min(slot + step));
            let local = zone.localize(slot);
            let key = day_slot(local);
            for pair in [forecast.daily.entry(key).or_default(), forecast.weekly.entry((local.weekday().number_days_from_monday(), key)).or_default()] {
                pair.0 += u32::from(active);
                pair.1 += 1;
            }
            slot += step;
        }
        forecast
    }

    pub fn probability(&self, at: Moment) -> f64 {
        let local = self.zone.localize(at);
        let key = day_slot(local);
        let ratio = |pair: Option<&(u32, u32)>| pair.filter(|pair| pair.1 > 0).map(|pair| f64::from(pair.0) / f64::from(pair.1));
        let day_p = ratio(self.daily.get(&key)).unwrap_or(0.0);
        let week = self.weekly.get(&(local.weekday().number_days_from_monday(), key));
        let weeks = week.map_or(0, |pair| pair.1);
        let weight = (f64::from(weeks) / 3.0).min(1.0);
        weight * ratio(week).unwrap_or(day_p) + (1.0 - weight) * day_p
    }
}
