//! Pure SHA-256 (FIPS 180-4) for the dependency-free core identity
//! primitive. Mirrors the Node-free TypeScript reference digest module
//! so the Rust core needs no cryptographic dependency; the differential
//! digest scenarios verify byte-for-byte parity with the oracle.

/// SHA-256 round constants (FIPS 180-4 section 4.2.2).
const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1,
    0x923f82a4, 0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
    0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786,
    0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147,
    0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
    0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
    0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a,
    0x5b9cca4f, 0x682e6ff3, 0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
    0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

fn rotate_right(value: u32, shift: u32) -> u32 {
    value.rotate_right(shift)
}

/// One FIPS 180-4 compression step over a full 64-byte message block.
fn compress(hash: &mut [u32; 8], block: &[u8]) {
    let mut words = [0u32; 64];
    for (index, word) in words.iter_mut().enumerate().take(16) {
        *word = u32::from_be_bytes([
            block[index * 4],
            block[index * 4 + 1],
            block[index * 4 + 2],
            block[index * 4 + 3],
        ]);
    }
    for index in 16..64 {
        let w15 = words[index - 15];
        let w2 = words[index - 2];
        let s0 = rotate_right(w15, 7) ^ rotate_right(w15, 18) ^ (w15 >> 3);
        let s1 = rotate_right(w2, 17) ^ rotate_right(w2, 19) ^ (w2 >> 10);
        words[index] = words[index - 16]
            .wrapping_add(s0)
            .wrapping_add(words[index - 7])
            .wrapping_add(s1);
    }
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *hash;
    for index in 0..64 {
        let s1 =
            rotate_right(e, 6) ^ rotate_right(e, 11) ^ rotate_right(e, 25);
        let ch = (e & f) ^ ((!e) & g);
        let temp1 = h
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(K[index])
            .wrapping_add(words[index]);
        let s0 =
            rotate_right(a, 2) ^ rotate_right(a, 13) ^ rotate_right(a, 22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let temp2 = s0.wrapping_add(maj);
        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(temp1);
        d = c;
        c = b;
        b = a;
        a = temp1.wrapping_add(temp2);
    }
    for (state, word) in hash.iter_mut().zip([a, b, c, d, e, f, g, h]) {
        *state = state.wrapping_add(word);
    }
}

/// Incremental SHA-256 state (`update`/`finish`) mirroring the
/// TypeScript oracle's streaming `createHash("sha256")`, so callers can
/// hash bounded chunks of a large input without buffering it whole.
#[derive(Debug, Clone)]
pub struct Sha256 {
    hash: [u32; 8],
    buffer: [u8; 64],
    buffered: usize,
    length_bits: u64,
}

impl Default for Sha256 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha256 {
    /// A fresh hasher in the FIPS 180-4 initial state.
    pub fn new() -> Self {
        Self {
            hash: [
                0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f,
                0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
            ],
            buffer: [0; 64],
            buffered: 0,
            length_bits: 0,
        }
    }

    /// Absorb the next input bytes.
    pub fn update(&mut self, bytes: &[u8]) -> &mut Self {
        self.length_bits = self
            .length_bits
            .wrapping_add((bytes.len() as u64).wrapping_mul(8));
        let mut input = bytes;
        if self.buffered > 0 && !input.is_empty() {
            let take = usize::min(64 - self.buffered, input.len());
            self.buffer[self.buffered..self.buffered + take]
                .copy_from_slice(&input[..take]);
            self.buffered += take;
            input = &input[take..];
            if self.buffered == 64 {
                let block = self.buffer;
                compress(&mut self.hash, &block);
                self.buffered = 0;
            }
        }
        while input.len() >= 64 {
            let (block, rest) = input.split_at(64);
            compress(&mut self.hash, block);
            input = rest;
        }
        if !input.is_empty() {
            self.buffer[..input.len()].copy_from_slice(input);
            self.buffered = input.len();
        }
        self
    }

    /// Pad the buffered tail (FIPS 180-4 section 5.1) and return the
    /// lowercase hex digest. The hasher keeps its state, but reuse after
    /// `finish` is not meaningful.
    pub fn finish(&self) -> String {
        let mut hash = self.hash;
        let mut tail = [0u8; 128];
        let used = self.buffered;
        tail[..used].copy_from_slice(&self.buffer[..used]);
        tail[used] = 0x80;
        let total = if used < 56 { 64 } else { 128 };
        tail[total - 8..total]
            .copy_from_slice(&self.length_bits.to_be_bytes());
        compress(&mut hash, &tail[..64]);
        if total == 128 {
            compress(&mut hash, &tail[64..128]);
        }
        let mut out = String::with_capacity(64);
        for word in hash {
            out.push_str(&format!("{word:08x}"));
        }
        out
    }
}

/// Hex SHA-256 digest of the exact input bytes.
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::new().update(bytes).finish()
}

#[cfg(test)]
mod tests {
    use super::{Sha256, sha256_hex};

    #[test]
    fn incremental_hashing_matches_the_one_shot_digest() {
        let input: Vec<u8> =
            (0..1000usize).map(|index| (index % 251) as u8).collect();
        for chunk in [1usize, 7, 55, 63, 64, 65, 127, 1000] {
            let mut hasher = Sha256::new();
            for part in input.chunks(chunk) {
                hasher.update(part);
            }
            assert_eq!(
                hasher.finish(),
                sha256_hex(&input),
                "chunk size {chunk}"
            );
        }
    }

    #[test]
    fn empty_updates_do_not_change_the_digest() {
        let mut hasher = Sha256::new();
        hasher.update(b"");
        hasher.update(b"abc");
        hasher.update(b"");
        assert_eq!(hasher.finish(), sha256_hex(b"abc"));
    }

    #[test]
    fn exact_block_and_tail_boundaries_stream_correctly() {
        for length in [0usize, 55, 56, 63, 64, 65, 119, 120, 128] {
            let input: Vec<u8> =
                (0..length).map(|index| (index % 251) as u8).collect();
            let mut hasher = Sha256::new();
            hasher.update(&input[..length.min(1)]);
            hasher.update(&input[length.min(1)..]);
            assert_eq!(hasher.finish(), sha256_hex(&input), "length {length}");
        }
    }
}
