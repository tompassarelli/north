pub(crate) fn cases() -> usize {
    if std::env::var_os("CI").is_some() { 2048 } else { 128 }
}

pub(crate) struct Generator(u64);

impl Generator {
    pub(crate) fn new(seed: u64) -> Self { Self(seed) }

    pub(crate) fn index(&mut self, bound: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % bound as u64) as usize
    }

    pub(crate) fn word(&mut self) -> String {
        let length = 1 + self.index(16);
        (0..length).map(|_| (b'a' + self.index(26) as u8) as char).collect()
    }
}
