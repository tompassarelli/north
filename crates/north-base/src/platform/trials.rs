use std::collections::BTreeMap;

pub type Scenarios = BTreeMap<String, [usize; 2]>;

pub const TRIALS: usize = 3;
const ALPHA: f64 = 0.05;

#[derive(Debug, PartialEq)]
pub enum Verdict {
    Drop,
    Gain,
    Same,
}

fn comb(n: usize, k: usize) -> f64 {
    let k = k.min(n - k.min(n));
    (1..=k).fold(1.0, |acc, i| acc * (n - k + i) as f64 / i as f64)
}

fn fisher_tail(a: usize, n1: usize, c: usize, n2: usize, upper: bool) -> f64 {
    let (k, total) = (a + c, n1 + n2);
    let (lo, hi) = (k.saturating_sub(n2), k.min(n1));
    let weight = |x: usize| comb(k, x) * comb(total - k, n1 - x);
    let tail: f64 = if upper { (a..=hi).map(weight).sum() } else { (lo..=a).map(weight).sum() };
    tail / comb(total, n1)
}

pub fn compare(point: (f64, usize), candidate: (f64, usize)) -> Verdict {
    let ((p, n1), (q, n2)) = (point, candidate);
    if n1 == 0 || n2 == 0 {
        return Verdict::Same;
    }
    let (a, c) = ((p * n1 as f64).round_ties_even() as usize, (q * n2 as f64).round_ties_even() as usize);
    if fisher_tail(a, n1, c, n2, true) < ALPHA {
        return Verdict::Drop;
    }
    if fisher_tail(a, n1, c, n2, false) < ALPHA {
        return Verdict::Gain;
    }
    Verdict::Same
}

pub fn flips(point: &Scenarios, candidate: &Scenarios) -> Vec<String> {
    candidate
        .iter()
        .filter(|(scenario, [passed, n])| {
            *n >= TRIALS && *passed == 0 && point.get(*scenario).is_some_and(|top| top[1] >= TRIALS && top[0] == top[1])
        })
        .map(|(scenario, _)| scenario.clone())
        .collect()
}
