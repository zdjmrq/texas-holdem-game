//! Bit-exact port of the original AI RNG (js/ai-core.js SeededRng).
//!
//! The AI RNG is NOT the probability-core xorshift32 generator: it is the
//! FNV-1a `hashSeed` plus the mulberry32-style `next()` from the original JS
//! AI. The exact stream is part of the golden-corpus contract.

/// Port of the original JS `hashSeed(value)` for AI seeds.
///
/// JS: `String(value ?? 'poker-ai')` then FNV-1a over UTF-16 code units,
/// falling back to `0x9e3779b9` when the hash is zero.
#[must_use]
pub fn hash_seed_ai(seed: &str) -> u32 {
    let mut hash: u32 = 2_166_136_261;
    for unit in seed.encode_utf16() {
        hash ^= u32::from(unit);
        hash = hash.wrapping_mul(16_777_619);
    }
    if hash == 0 { 0x9e37_79b9 } else { hash }
}

/// Deterministic AI RNG matching the original JS SeededRng bit for bit.
///
/// Each decision creates a fresh stream seeded from
/// `<seed>:<handId>:<seatId>:<decisionId>:<style>`. `calls` counts
/// `next_f64()` invocations and is used by the golden corpus replay
/// (`expect.rngCalls`).
#[derive(Clone, Debug)]
pub struct AiRng {
    state: u32,
    calls: u64,
}

impl AiRng {
    /// Creates a generator from a seed string.
    #[must_use]
    pub fn from_seed(seed: &str) -> Self {
        Self {
            state: hash_seed_ai(seed),
            calls: 0,
        }
    }

    /// Creates a generator from a raw state word.
    #[must_use]
    pub const fn from_state(state: u32) -> Self {
        Self { state, calls: 0 }
    }

    /// Number of `next_f64` calls made on this stream.
    #[must_use]
    pub const fn calls(&self) -> u64 {
        self.calls
    }

    /// Next uniform float in `[0, 1)` (JS `SeededRng.next()`).
    #[allow(clippy::cast_precision_loss)]
    pub fn next_f64(&mut self) -> f64 {
        self.calls += 1;
        self.state = self.state.wrapping_add(0x6D2B_79F5);
        let mut t = self.state;
        t = (t ^ (t >> 15)).wrapping_mul(t | 1);
        t ^= t.wrapping_add((t ^ (t >> 7)).wrapping_mul(t | 61));
        f64::from(t ^ (t >> 14)) / 4_294_967_296.0
    }

    /// Uniform integer in `[0, max)` (JS `SeededRng.int`).
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    pub fn int(&mut self, max: usize) -> usize {
        let bound = max.max(1);
        (self.next_f64() * bound as f64) as usize
    }

    /// Standard normal sample (JS `SeededRng.normal`); unused by production
    /// decisions but kept for parity and future experiments.
    pub fn normal(&mut self) -> f64 {
        let u = self.next_f64().max(1e-9);
        let v = self.next_f64();
        (-2.0 * u.ln()).sqrt() * (2.0 * std::f64::consts::PI * v).cos()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_seed_matches_js_vectors() {
        assert_eq!(hash_seed_ai("table:1:0:1:SOLID"), 2_490_085_092);
        assert_eq!(
            hash_seed_ai("poker-table:standard:12:3:standard:river:5:3:TAG"),
            3_525_023_011
        );
        assert_eq!(
            hash_seed_ai("poker-table:shortdeck:4:2:shortdeck:flop:1:2:LAG"),
            1_746_015_351
        );
    }

    #[test]
    fn next_matches_js_vectors() {
        let mut rng = AiRng::from_seed("table:1:0:1:SOLID");
        let expected = [
            0.534_079_151_693_731_5,
            0.890_896_503_580_734_1,
            0.136_854_010_866_954_92,
            0.111_601_880_053_058_27,
            0.837_332_837_050_780_7,
            0.814_853_851_683_437_8,
            0.953_026_423_696_428_5,
            0.670_235_959_347_337_5,
        ];
        for value in expected {
            assert_eq!(rng.next_f64(), value);
        }
        assert_eq!(rng.calls(), 8);
    }
}
