//! The task-cluster bootstrap, drawing from the generator the Python report seeded so seed 0 gives the same interval.
use std::collections::BTreeMap;

const N: usize = 624;
const M: usize = 397;
const UPPER: u32 = 0x8000_0000;
const LOWER: u32 = 0x7fff_ffff;

pub(super) struct PyRandom {
    state: [u32; N],
    index: usize,
}

impl PyRandom {
    pub(super) fn seeded_zero() -> Self {
        let mut state = [0u32; N];
        state[0] = 19_650_218;
        for i in 1..N {
            state[i] = 1_812_433_253u32.wrapping_mul(state[i - 1] ^ (state[i - 1] >> 30)).wrapping_add(i as u32);
        }
        let mut i = 1;
        for _ in 0..N {
            state[i] ^= (state[i - 1] ^ (state[i - 1] >> 30)).wrapping_mul(1_664_525);
            i += 1;
            if i >= N {
                state[0] = state[N - 1];
                i = 1;
            }
        }
        for _ in 0..N - 1 {
            state[i] = (state[i] ^ (state[i - 1] ^ (state[i - 1] >> 30)).wrapping_mul(1_566_083_941)).wrapping_sub(i as u32);
            i += 1;
            if i >= N {
                state[0] = state[N - 1];
                i = 1;
            }
        }
        state[0] = UPPER;
        Self { state, index: N }
    }

    fn twist(&mut self) {
        for k in 0..N {
            let y = (self.state[k] & UPPER) | (self.state[(k + 1) % N] & LOWER);
            self.state[k] = self.state[(k + M) % N] ^ (y >> 1) ^ if y & 1 == 1 { 0x9908_b0df } else { 0 };
        }
        self.index = 0;
    }

    pub(super) fn next_u32(&mut self) -> u32 {
        if self.index >= N {
            self.twist();
        }
        let mut y = self.state[self.index];
        self.index += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^ (y >> 18)
    }

    pub(super) fn below(&mut self, n: usize) -> usize {
        let bits = usize::BITS - n.leading_zeros();
        loop {
            let draw = (self.next_u32() >> (32 - bits)) as usize;
            if draw < n {
                return draw;
            }
        }
    }
}

/// Python's `sum` over floats: Neumaier-compensated since 3.12.
pub(super) fn float_sum(values: impl IntoIterator<Item = f64>) -> f64 {
    let (mut total, mut carry) = (0.0f64, 0.0f64);
    for x in values {
        let t = total + x;
        carry += if total.abs() >= x.abs() { (total - t) + x } else { (x - t) + total };
        total = t;
    }
    if carry != 0.0 && carry.is_finite() { total + carry } else { total }
}

pub(super) fn upper_quantile(sorted: &[f64]) -> f64 {
    sorted[((0.975 * sorted.len() as f64) as usize).min(sorted.len() - 1)]
}

pub(super) struct Row {
    pub(super) scenario: String,
    pub(super) passed: Option<bool>,
}

fn task_means(rows: &[Row]) -> BTreeMap<&str, f64> {
    let mut cells: BTreeMap<&str, (u32, u32)> = BTreeMap::new();
    for row in rows {
        if let Some(passed) = row.passed {
            let cell = cells.entry(row.scenario.as_str()).or_default();
            cell.0 += u32::from(passed);
            cell.1 += 1;
        }
    }
    cells.into_iter().map(|(scenario, (won, n))| (scenario, f64::from(won) / f64::from(n))).collect()
}

pub(super) fn paired_diff(a: &[Row], b: &[Row], resamples: usize) -> (f64, f64, f64) {
    let (ma, mb) = (task_means(a), task_means(b));
    let diffs: Vec<f64> = ma.iter().filter_map(|(scenario, x)| mb.get(scenario).map(|y| x - y)).collect();
    if diffs.is_empty() {
        return (0.0, -1.0, 1.0);
    }
    let mut rng = PyRandom::seeded_zero();
    let k = diffs.len();
    let mut boot: Vec<f64> = (0..resamples).map(|_| float_sum((0..k).map(|_| diffs[rng.below(k)])) / k as f64).collect();
    boot.sort_by(f64::total_cmp);
    (float_sum(diffs.iter().copied()) / k as f64, boot[(0.025 * resamples as f64) as usize], upper_quantile(&boot))
}
