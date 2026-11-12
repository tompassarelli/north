pub fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

pub fn derive(master: u64, parts: &[&str]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325 ^ master;
    for part in parts {
        for byte in part.bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0100_0000_01b3);
        }
        hash ^= 0xff;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    splitmix(&mut hash)
}

pub fn shuffled<T: Clone>(items: &[T], master: u64, parts: &[&str]) -> Vec<T> {
    let mut state = derive(master, parts);
    let mut out = items.to_vec();
    for index in (1..out.len()).rev() {
        let pick = usize::try_from(splitmix(&mut state) % (index as u64 + 1)).unwrap_or(0);
        out.swap(index, pick);
    }
    out
}
