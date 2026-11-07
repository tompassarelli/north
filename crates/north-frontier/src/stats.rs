//! The interval arithmetic of the frontier scoreboard (docs/rationale/frontier.md): Wilson per arm,
//! the exact McNemar test with Newcombe's paired interval for the difference, and a seeded bootstrap.
const Z: f64 = 1.959_964;
const RESAMPLES: usize = 10_000;
const SEED: u64 = 730;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Interval {
    pub low: f64,
    pub high: f64,
}

pub fn wilson(hits: usize, of: usize) -> Option<Interval> {
    if of == 0 {
        return None;
    }
    let (n, p) = (of as f64, hits as f64 / of as f64);
    let denominator = 1.0 + Z * Z / n;
    let centre = (p + Z * Z / (2.0 * n)) / denominator;
    let half = Z * (p * (1.0 - p) / n + Z * Z / (4.0 * n * n)).sqrt() / denominator;
    Some(Interval { low: (centre - half).max(0.0), high: (centre + half).min(1.0) })
}

/// Counts of tasks by whether each arm was correct on them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pairs {
    pub both: usize,
    pub only_first: usize,
    pub only_second: usize,
    pub neither: usize,
}

impl Pairs {
    pub fn tasks(self) -> usize {
        self.both + self.only_first + self.only_second + self.neither
    }
}

/// The two-sided exact McNemar p-value: the discordant tasks split evenly under no difference.
pub fn mcnemar(pairs: Pairs) -> f64 {
    let discordant = pairs.only_first + pairs.only_second;
    if discordant == 0 {
        return 1.0;
    }
    let smaller = pairs.only_first.min(pairs.only_second);
    let mut term = 0.5_f64.powi(discordant as i32);
    let mut tail = 0.0;
    for k in 0..=smaller {
        tail += term;
        term = term * (discordant - k) as f64 / (k + 1) as f64;
    }
    (2.0 * tail).min(1.0)
}

/// Newcombe's method 10: the first arm's rate minus the second's over paired tasks, with its interval.
pub fn newcombe_paired(pairs: Pairs) -> Option<(f64, Interval)> {
    let n = pairs.tasks();
    let (a, b, c, d) = (pairs.both as f64, pairs.only_first as f64, pairs.only_second as f64, pairs.neither as f64);
    let first = wilson(pairs.both + pairs.only_first, n)?;
    let second = wilson(pairs.both + pairs.only_second, n)?;
    let (p1, p2) = ((a + b) / n as f64, (a + c) / n as f64);
    let margins = (a + b) * (c + d) * (a + c) * (b + d);
    let phi = if margins > 0.0 { (a * d - b * c) / margins.sqrt() } else { 0.0 };
    let reach = |x: f64, y: f64| (x * x + y * y - 2.0 * phi * x * y).max(0.0).sqrt();
    let delta = p1 - p2;
    Some((delta, Interval { low: delta - reach(p1 - first.low, second.high - p2), high: delta + reach(first.high - p1, p2 - second.low) }))
}

struct Splitmix(u64);

impl Splitmix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}

/// A percentile bootstrap over `items` with a fixed seed; resamples whose statistic is undefined are dropped.
pub fn bootstrap<T>(items: &[T], statistic: impl Fn(&[&T]) -> Option<f64>) -> Option<Interval> {
    if items.is_empty() {
        return None;
    }
    let mut random = Splitmix(SEED);
    let mut drawn: Vec<f64> = (0..RESAMPLES)
        .filter_map(|_| {
            let sample: Vec<&T> = (0..items.len()).map(|_| &items[random.below(items.len())]).collect();
            statistic(&sample)
        })
        .collect();
    if drawn.is_empty() {
        return None;
    }
    drawn.sort_by(f64::total_cmp);
    let at = |share: f64| drawn[((drawn.len() - 1) as f64 * share).round() as usize];
    Some(Interval { low: at(0.025), high: at(0.975) })
}

pub fn median(values: &[f64]) -> Option<f64> {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let middle = sorted.len() / 2;
    match sorted.len() {
        0 => None,
        length if length % 2 == 1 => Some(sorted[middle]),
        _ => Some((sorted[middle - 1] + sorted[middle]) / 2.0),
    }
}

pub fn mean(values: &[f64]) -> Option<f64> {
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}
