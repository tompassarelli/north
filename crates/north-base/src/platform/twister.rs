const N: usize = 624;
const M: usize = 397;

pub struct Twister {
    state: [u32; N],
    index: usize,
}

impl Twister {
    // Python's random.Random(seed) for a small non-negative int seed, so bootstrap rows match the measured sources.
    pub fn seeded(seed: u32) -> Self {
        let mut state = [0u32; N];
        state[0] = 19_650_218;
        for i in 1..N {
            state[i] = 1_812_433_253u32.wrapping_mul(state[i - 1] ^ (state[i - 1] >> 30)).wrapping_add(i as u32);
        }
        let (mut i, mut j) = (1usize, 0usize);
        for _ in 0..N {
            state[i] = (state[i] ^ (state[i - 1] ^ (state[i - 1] >> 30)).wrapping_mul(1_664_525)).wrapping_add(seed).wrapping_add(j as u32);
            i += 1;
            j = 0;
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
        state[0] = 0x8000_0000;
        Self { state, index: N }
    }

    fn next_u32(&mut self) -> u32 {
        if self.index >= N {
            for k in 0..N {
                let y = (self.state[k] & 0x8000_0000) | (self.state[(k + 1) % N] & 0x7fff_ffff);
                self.state[k] = self.state[(k + M) % N] ^ (y >> 1) ^ if y & 1 == 1 { 0x9908_b0df } else { 0 };
            }
            self.index = 0;
        }
        let mut y = self.state[self.index];
        self.index += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^ (y >> 18)
    }

    fn random(&mut self) -> f64 {
        let (a, b) = (self.next_u32() >> 5, self.next_u32() >> 6);
        (f64::from(a) * 67_108_864.0 + f64::from(b)) / 9_007_199_254_740_992.0
    }

    pub fn choices(&mut self, population: &[f64], k: usize) -> Vec<f64> {
        let n = population.len() as f64;
        (0..k).map(|_| population[(self.random() * n).floor() as usize]).collect()
    }
}
