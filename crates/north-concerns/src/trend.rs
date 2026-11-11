use super::history::State;
use super::registry::{Bound, Class, Concern};

const DAY: f64 = 86_400.0;
const HOUR: i64 = 3_600;
const CROSSING_DAYS: f64 = 7.0;
const MIN_SAMPLES: usize = 6;
const MAD_TO_SIGMA: f64 = 1.4826;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub at: i64,
    pub value: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct Window {
    pub name: &'static str,
    pub seconds: i64,
    hourly: bool,
}

pub const WINDOWS: [Window; 3] = [Window { name: "12 h", seconds: 12 * HOUR, hourly: false }, Window { name: "24 h", seconds: 24 * HOUR, hourly: false }, Window { name: "7 d", seconds: 7 * 24 * HOUR, hourly: true }];
pub const LONGEST: i64 = 7 * 24 * HOUR;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fit {
    pub median: f64,
    pub slope: f64,
    noise: f64,
    enough: bool,
}

impl Fit {
    pub fn per_day(&self) -> f64 {
        self.slope * DAY
    }

    fn significant(&self, window: &Window) -> bool {
        self.enough && (self.slope * window.seconds as f64).abs() > 2.0 * self.noise
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Judged {
    pub state: State,
    pub meets: bool,
    pub fits: [Option<Fit>; 3],
}

struct Line {
    down_is_better: bool,
    red_at: Option<f64>,
    target: f64,
}

impl Line {
    fn of(concern: &Concern) -> Self {
        let down_is_better = matches!(concern.bound, Bound::Max(_));
        let red_at = match concern.class {
            Class::Debt => concern.debt.as_ref().map(|debt| debt.ceiling),
            Class::Contract | Class::Ready => Some(concern.bound.limit()),
        };
        Self { down_is_better, red_at, target: concern.bound.limit() }
    }

    fn worse(&self, slope: f64) -> bool {
        if self.down_is_better { slope > 0.0 } else { slope < 0.0 }
    }

    fn past(&self, line: f64, value: f64) -> bool {
        if self.down_is_better { value > line } else { value < line }
    }

    fn within_a_week(&self, line: f64, value: f64, slope: f64) -> bool {
        self.worse(slope) && (line - value) / (slope * DAY) <= CROSSING_DAYS
    }
}

pub fn judge(concern: &Concern, current: f64, history: &[Point], now: i64) -> Judged {
    let line = Line::of(concern);
    let magnitude = line.red_at.unwrap_or(line.target).abs();
    let fits = WINDOWS.map(|window| fit(history, now, &window, magnitude));
    let [twelve, day, week] = fits;
    let persisted = |fit: Option<Fit>, window: &Window| fit.filter(|fit| fit.significant(window) && twelve.zip(day).is_some_and(|(short, long)| short.slope.signum() == long.slope.signum()));
    let (short, long, slow) = (persisted(twelve, &WINDOWS[0]), persisted(day, &WINDOWS[1]), week.filter(|fit| fit.significant(&WINDOWS[2])));
    let state = match line.red_at {
        Some(red_at) if line.past(red_at, current) => {
            if short.is_some_and(|fit| !line.worse(fit.slope)) { State::Repairing } else { State::Red }
        }
        Some(red_at) => {
            let drifting = [long, slow].into_iter().flatten().any(|fit| line.within_a_week(red_at, current, fit.slope));
            if drifting { State::Drifting } else { State::Green }
        }
        None => {
            if long.is_some_and(|fit| line.worse(fit.slope)) { State::Drifting } else { State::Green }
        }
    };
    Judged { state, meets: concern.bound.holds(current), fits }
}

pub fn settle(shown: State, last: State, now: State) -> State {
    let sticky = matches!(shown, State::Drifting | State::Repairing | State::Red);
    if sticky && now != shown && now != last { shown } else { now }
}

fn fit(history: &[Point], now: i64, window: &Window, magnitude: f64) -> Option<Fit> {
    let inside: Vec<Point> = history.iter().filter(|point| point.at > now - window.seconds && point.at <= now).copied().collect();
    let series = if window.hourly { hourly(&inside, now - window.seconds) } else { inside };
    let (first, last) = (series.first()?, series.last()?);
    let mut slopes: Vec<f64> = Vec::new();
    for (index, early) in series.iter().enumerate() {
        for late in series.iter().skip(index + 1).filter(|late| late.at != early.at) {
            slopes.push((late.value - early.value) / (late.at - early.at) as f64);
        }
    }
    let slope = median(&mut slopes)?;
    let offset = |point: &Point| (point.at - now) as f64;
    let intercept = median(&mut series.iter().map(|point| point.value - slope * offset(point)).collect::<Vec<_>>())?;
    let mut deviations: Vec<f64> = series.iter().map(|point| (point.value - intercept - slope * offset(point)).abs()).collect();
    let whole = series.iter().all(|point| point.value.fract() == 0.0);
    let floor = if whole { 1.0 } else { 0.01 * magnitude };
    let noise = (MAD_TO_SIGMA * median(&mut deviations)?).max(floor);
    let enough = series.len() >= MIN_SAMPLES && (last.at - first.at) * 2 >= window.seconds;
    Some(Fit { median: median(&mut series.iter().map(|point| point.value).collect::<Vec<_>>())?, slope, noise, enough })
}

fn hourly(points: &[Point], start: i64) -> Vec<Point> {
    let mut buckets: std::collections::BTreeMap<i64, Vec<f64>> = std::collections::BTreeMap::new();
    for point in points {
        buckets.entry((point.at - start) / HOUR).or_default().push(point.value);
    }
    buckets.into_iter().filter_map(|(hour, mut values)| Some(Point { at: start + hour * HOUR + HOUR / 2, value: median(&mut values)? })).collect()
}

fn median(values: &mut [f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    Some(if values.len().is_multiple_of(2) { (values[middle - 1] + values[middle]) / 2.0 } else { values[middle] })
}
