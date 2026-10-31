const IV: [u64; 8] = [
    0x6a09e667f3bcc908, 0xbb67ae8584caa73b, 0x3c6ef372fe94f82b, 0xa54ff53a5f1d36f1,
    0x510e527fade682d1, 0x9b05688c2b3e6c1f, 0x1f83d9abfb41bd6b, 0x5be0cd19137e2179,
];

const SIGMA: [[usize; 16]; 10] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
    [11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4],
    [7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8],
    [9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13],
    [2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9],
    [12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11],
    [13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 4, 8, 6, 2, 10],
    [6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5],
    [10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0],
];

fn mix(v: &mut [u64; 16], lanes: [usize; 4], x: u64, y: u64) {
    let [a, b, c, d] = lanes;
    v[a] = v[a].wrapping_add(v[b]).wrapping_add(x);
    v[d] = (v[d] ^ v[a]).rotate_right(32);
    v[c] = v[c].wrapping_add(v[d]);
    v[b] = (v[b] ^ v[c]).rotate_right(24);
    v[a] = v[a].wrapping_add(v[b]).wrapping_add(y);
    v[d] = (v[d] ^ v[a]).rotate_right(16);
    v[c] = v[c].wrapping_add(v[d]);
    v[b] = (v[b] ^ v[c]).rotate_right(63);
}

fn compress(state: &mut [u64; 8], block: &[u8], counter: u128, last: bool) {
    let words: Vec<u64> = block.chunks_exact(8).map(|part| u64::from_le_bytes(part.try_into().unwrap_or([0; 8]))).collect();
    let mut v = [0u64; 16];
    v[..8].copy_from_slice(state);
    v[8..].copy_from_slice(&IV);
    v[12] ^= counter as u64;
    v[13] ^= (counter >> 64) as u64;
    if last {
        v[14] = !v[14];
    }
    for round in 0..12 {
        let s = &SIGMA[round % 10];
        mix(&mut v, [0, 4, 8, 12], words[s[0]], words[s[1]]);
        mix(&mut v, [1, 5, 9, 13], words[s[2]], words[s[3]]);
        mix(&mut v, [2, 6, 10, 14], words[s[4]], words[s[5]]);
        mix(&mut v, [3, 7, 11, 15], words[s[6]], words[s[7]]);
        mix(&mut v, [0, 5, 10, 15], words[s[8]], words[s[9]]);
        mix(&mut v, [1, 6, 11, 12], words[s[10]], words[s[11]]);
        mix(&mut v, [2, 7, 8, 13], words[s[12]], words[s[13]]);
        mix(&mut v, [3, 4, 9, 14], words[s[14]], words[s[15]]);
    }
    for index in 0..8 {
        state[index] ^= v[index] ^ v[index + 8];
    }
}

/// BLAKE2b with an 8-byte digest, read as a signed big-endian integer: the key the index stores for each distinct text.
pub(super) fn text_hash(text: &str) -> i64 {
    let data = text.as_bytes();
    let mut state = IV;
    state[0] ^= 0x0101_0000 ^ 8;
    let mut offset = 0;
    while data.len() - offset > 128 {
        compress(&mut state, &data[offset..offset + 128], (offset + 128) as u128, false);
        offset += 128;
    }
    let mut block = [0u8; 128];
    block[..data.len() - offset].copy_from_slice(&data[offset..]);
    compress(&mut state, &block, data.len() as u128, true);
    let mut digest = [0u8; 8];
    digest.copy_from_slice(&state[0].to_le_bytes());
    i64::from_be_bytes(digest)
}
