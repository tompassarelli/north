//! Replays GNU Awk's srand and rand draw for draw, so a reading the awk reader registered reproduces.
use std::collections::BTreeMap;

const RESAMPLES: usize = 10_000;
const FAR_AHEAD: f64 = 0.67;

pub type Samples = BTreeMap<String, Vec<f64>>;

struct AwkRandom {
    state: [u32; 63],
    shuffle: [u32; 512],
    pick: Option<u32>,
    front: usize,
    rear: usize,
}

impl AwkRandom {
    fn new(seed: u32) -> Self {
        let mut state = [0; 63];
        let mut word = seed;
        for slot in &mut state {
            *slot = word;
            let x = match word as i32 {
                0 => 123_459_876,
                x => x,
            };
            let next = 16_807 * (x % 127_773) - 2_836 * (x / 127_773);
            word = (if next < 0 { next + 0x7fff_ffff } else { next }) as u32;
        }
        let mut random = Self { state, front: 1, rear: 0, shuffle: [0; 512], pick: None };
        for _ in 0..10 * random.state.len() {
            random.word();
        }
        random
    }

    fn word(&mut self) -> u32 {
        let pick = match self.pick {
            Some(pick) => pick,
            None => {
                self.shuffle = std::array::from_fn(|_| self.step());
                self.step()
            }
        };
        let next = self.step();
        let slot = pick as usize % self.shuffle.len();
        let word = std::mem::replace(&mut self.shuffle[slot], next);
        self.pick = Some(word);
        word
    }

    fn step(&mut self) -> u32 {
        self.state[self.front] = self.state[self.front].wrapping_add(self.state[self.rear]);
        let word = self.state[self.front] >> 1;
        self.front += 1;
        self.rear += 1;
        if self.front == self.state.len() {
            self.front = 0;
        } else if self.rear == self.state.len() {
            self.rear = 0;
        }
        word
    }

    fn uniform(&mut self) -> f64 {
        const SPAN: f64 = 2_147_483_648.0;
        loop {
            let (low, high) = (f64::from(self.word()), f64::from(self.word()));
            let value = 0.5 + (low / SPAN + high) / SPAN - 0.5;
            if value != 1.0 {
                return value;
            }
        }
    }

    fn index(&mut self, len: usize) -> usize {
        (self.uniform() * len as f64) as usize
    }
}

pub fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    if values.is_empty() {
        f64::NAN
    } else if values.len() % 2 == 0 {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    }
}

fn mean(times: &[f64], random: &mut Option<AwkRandom>) -> f64 {
    let mut sum = 0.0;
    for i in 0..times.len() {
        sum += times[random.as_mut().map_or(i, |random| random.index(times.len()))];
    }
    sum / times.len() as f64
}

fn ratio(order: &[&str], mine: &Samples, theirs: &Samples, random: &mut Option<AwkRandom>) -> Option<f64> {
    let mut ratios = Vec::with_capacity(order.len());
    for k in 0..order.len() {
        let task = order[random.as_mut().map_or(k, |random| random.index(order.len()))];
        let north = mean(mine.get(task)?, random);
        ratios.push(north / mean(theirs.get(task)?, random));
    }
    Some(median(ratios))
}

pub fn interval(order: &[&str], mine: &Samples, theirs: &Samples, seed: u32) -> Option<(f64, f64)> {
    let mut random = Some(AwkRandom::new(seed));
    let mut resampled = (0..RESAMPLES).map(|_| ratio(order, mine, theirs, &mut random)).collect::<Option<Vec<f64>>>()?;
    resampled.sort_by(f64::total_cmp);
    Some((resampled[RESAMPLES / 10], resampled[RESAMPLES * 9 / 10]))
}

pub fn band(order: &[&str], theirs: &Samples) -> Option<f64> {
    order
        .iter()
        .map(|task| {
            let times = theirs.get(*task)?;
            Some(times.iter().copied().fold(f64::MIN, f64::max) / times.iter().copied().fold(f64::MAX, f64::min))
        })
        .collect::<Option<Vec<f64>>>()
        .map(median)
}

pub fn drift(order: &[&str], theirs: &Samples) -> Option<&'static str> {
    let every = |step: fn(f64, f64) -> bool| order.iter().all(|task| theirs.get(*task).is_some_and(|times| times.windows(2).all(|pair| step(pair[0], pair[1]))));
    if every(|before, after| after > before) {
        Some("slower")
    } else if every(|before, after| after < before) {
        Some("faster")
    } else {
        None
    }
}

pub fn label(ratio: f64, (low, high): (f64, f64), band: f64) -> String {
    let inner = 1.0 / band;
    let (word, crosses) = if ratio <= FAR_AHEAD && ratio < inner {
        ("far ahead", high > FAR_AHEAD.min(inner))
    } else if ratio < inner {
        ("ahead", high >= inner)
    } else if ratio > band {
        ("behind", low <= band)
    } else {
        ("inside the band", low < inner || high > band)
    };
    if crosses { format!("{word}, low confidence") } else { word.to_owned() }
}
