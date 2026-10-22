//! Injectable randomness.
//!
//! The core crates never touch the thread RNG directly: every shuffle, AI
//! mixture and Monte Carlo sample takes a [`RandomSource`] parameter. That
//! keeps runs reproducible for golden tests and lets the probability engine
//! reproduce the original JS sample stream bit-for-bit via
//! [`XorShift32Rng`].

use rand::RngCore;
use rand::SeedableRng;
use rand_chacha::ChaCha8Rng;

/// Minimal RNG interface used across the poker crates.
pub trait RandomSource {
    /// Next 32 random bits.
    fn next_u32(&mut self) -> u32;

    /// Uniform float in [0, 1).
    ///
    /// The exact bit pattern is part of the golden-corpus contract for
    /// [`XorShift32Rng`]: it reproduces the original JS
    /// probability-core randomFactory output exactly.
    fn next_f64(&mut self) -> f64;
}

/// Port of the original JS hashSeed helper (FNV-1a over UTF-16 code units).
#[must_use]
pub fn hash_seed(seed: &str) -> u32 {
    // JS uses String(value || 'poker-probability'), so an empty seed falls back
    // to the default seed string.
    let text = if seed.is_empty() {
        "poker-probability"
    } else {
        seed
    };
    let mut hash: u32 = 2_166_136_261;
    for unit in text.encode_utf16() {
        hash ^= u32::from(unit);
        hash = hash.wrapping_mul(16_777_619);
    }
    if hash == 0 { 0x9e37_79b9 } else { hash }
}

/// xorshift32 generator compatible with the original JS probability core.
///
/// This is deliberately not a cryptographically secure generator: it exists to
/// reproduce the JS Monte Carlo sample stream and to provide fast deterministic
/// test randomness.
#[derive(Clone, Debug)]
pub struct XorShift32Rng {
    state: u32,
}

impl XorShift32Rng {
    /// Creates a generator from a seed string, matching JS hashSeed.
    #[must_use]
    pub fn from_seed(seed: &str) -> Self {
        Self {
            state: hash_seed(seed),
        }
    }

    /// Creates a generator from a raw state word.
    #[must_use]
    pub const fn from_state(state: u32) -> Self {
        Self { state }
    }

    /// Current internal state.
    #[must_use]
    pub const fn state(&self) -> u32 {
        self.state
    }
}

impl RandomSource for XorShift32Rng {
    fn next_u32(&mut self) -> u32 {
        self.state ^= self.state << 13;
        self.state ^= self.state >> 17;
        self.state ^= self.state << 5;
        self.state
    }

    fn next_f64(&mut self) -> f64 {
        f64::from(self.next_u32()) / 4_294_967_296.0
    }
}

/// Adapter that exposes a ChaCha8 generator as a [`RandomSource`].
#[derive(Clone, Debug)]
pub struct ChaChaAdapter {
    rng: ChaCha8Rng,
}

impl ChaChaAdapter {
    /// Creates an adapter from a 32-byte seed.
    #[must_use]
    pub fn from_seed(seed: [u8; 32]) -> Self {
        Self {
            rng: ChaCha8Rng::from_seed(seed),
        }
    }

    /// Creates an adapter from a u64 seed (used by simulation tooling).
    #[must_use]
    pub fn from_u64(seed: u64) -> Self {
        Self {
            rng: ChaCha8Rng::seed_from_u64(seed),
        }
    }
}

impl RandomSource for ChaChaAdapter {
    fn next_u32(&mut self) -> u32 {
        self.rng.next_u32()
    }

    fn next_f64(&mut self) -> f64 {
        // 53 random bits scaled into [0, 1), the usual f64 mantissa precision.
        let bits = self.rng.next_u64() >> 11;
        (bits as f64) * (1.0 / ((1u64 << 53) as f64))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_hash_seed_matches_known_values() {
        // Reference values produced by the original js/probability-core.js
        // hashSeed() implementation.
        assert_eq!(hash_seed("poker-probability"), 2_588_662_556);
        assert_eq!(hash_seed("aces-opponent-count"), 3_618_567_475);
        assert_eq!(hash_seed("golden-seed"), 1_950_149_494);
        // Empty seeds fall back to the default seed string in JS.
        assert_eq!(hash_seed(""), 2_588_662_556);
    }

    #[test]
    fn js_f64_stream_matches_original_probability_core() {
        // First ten samples of the original JS randomFactory('golden-seed').
        let expected = [
            0.762_110_009_556_636_2,
            0.016_994_591_802_358_627,
            0.689_882_733_626_291_2,
            0.724_181_143_334_135_4,
            0.332_531_634_951_010_35,
            0.766_043_376_177_549_4,
            0.526_776_212_733_238_9,
            0.240_224_430_104_717_6,
            0.113_793_569_151_312_11,
            0.774_663_518_648_594_6,
        ];
        let mut rng = XorShift32Rng::from_seed("golden-seed");
        for (index, want) in expected.iter().enumerate() {
            let got = rng.next_f64();
            assert_eq!(got, *want, "sample {index}");
        }
    }

    #[test]
    fn xorshift32_stream_is_deterministic() {
        let mut a = XorShift32Rng::from_seed("golden-seed");
        let mut b = XorShift32Rng::from_seed("golden-seed");
        for _ in 0..1000 {
            assert_eq!(a.next_u32(), b.next_u32());
        }
    }

    #[test]
    fn next_f64_stays_in_unit_interval() {
        let mut rng = XorShift32Rng::from_seed("unit-interval");
        for _ in 0..10_000 {
            let value = rng.next_f64();
            assert!((0.0..1.0).contains(&value), "out of range: {value}");
        }
    }
}
