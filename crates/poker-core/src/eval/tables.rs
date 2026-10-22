//! Precomputed lookup tables for the fast complete-hand evaluator.
//!
//! Design:
//!
//! * A seven-card hand is reduced to per-rank counts packed as four-bit
//!   nibbles in a `u64` (rank 2 is nibble 0, ... ace is nibble 12).
//! * When no suit has five or more cards, the best five-card hand depends only
//!   on that histogram. All 73,775 legal five/six/seven-card rank patterns are
//!   enumerated once (lazily, behind `OnceLock`) and stored in a bucket-sorted
//!   table keyed by the packed histogram.
//! * Flush and straight-flush evaluation uses 8,192-entry tables over the
//!   13-bit rank mask of the flush suit.
//!
//! Everything is immutable after initialization; no `unsafe` is used (the
//! crate keeps `#![forbid(unsafe_code)]`). Table entries store the semantic
//! hand category plus up to five kicker values; the public score is still
//! assembled by `eval::finish`, so `rank_index`, `score` and `name` remain
//! bit-identical to the reference implementation.

use std::sync::OnceLock;

use crate::card::Card;

use super::HandCategory;

/// Number of distinct ranks in either variant.
pub(super) const RANK_COUNT: usize = 13;
/// Number of 13-bit rank masks.
pub(super) const MASK_COUNT: usize = 1 << RANK_COUNT;
const HASH_BITS: u32 = 16;
const BUCKET_COUNT: usize = 1 << HASH_BITS;
const HASH_SHIFT: u32 = 64 - HASH_BITS;

/// Per-card rank-count increment: `1 << (4 * rank_index)`.
pub(super) const CARD_COUNT: [u64; 52] = build_card_count();
/// Per-card suit index (0..3).
pub(super) const CARD_SUIT: [u8; 52] = build_card_suit();
/// Per-card rank bit (rank 2 -> bit 0, ... ace -> bit 12).
pub(super) const CARD_RANK_BIT: [u16; 52] = build_card_rank_bit();

const fn card_at(index: u8) -> Card {
    match Card::from_index(index) {
        Some(card) => card,
        None => panic!("card index out of range"),
    }
}

const fn build_card_count() -> [u64; 52] {
    let mut table = [0u64; 52];
    let mut index = 0usize;
    while index < 52 {
        let card = card_at(index as u8);
        table[index] = 1u64 << (((card.rank_value() - 2) as u32) * 4);
        index += 1;
    }
    table
}

const fn build_card_suit() -> [u8; 52] {
    let mut table = [0u8; 52];
    let mut index = 0usize;
    while index < 52 {
        let card = card_at(index as u8);
        table[index] = card.suit() as u8;
        index += 1;
    }
    table
}

const fn build_card_rank_bit() -> [u16; 52] {
    let mut table = [0u16; 52];
    let mut index = 0usize;
    while index < 52 {
        let card = card_at(index as u8);
        table[index] = 1u16 << (card.rank_value() - 2);
        index += 1;
    }
    table
}

/// Best straight high card in a 13-bit rank mask, or 0 when there is none.
/// Mirrors the JS `straightHigh()` rank rules (standard wheel / short-deck
/// wheel included).
fn straight_high_from_mask(mask: u16, short_deck: bool) -> u8 {
    let lowest_high: u8 = if short_deck { 10 } else { 6 };
    let mut high = 14u8;
    while high >= lowest_high {
        let mut offset = 0u8;
        let mut present = true;
        while offset < 5 {
            let bit = high - offset - 2;
            if mask & (1u16 << bit) == 0 {
                present = false;
                break;
            }
            offset += 1;
        }
        if present {
            return high;
        }
        high -= 1;
    }
    if short_deck {
        // Short-deck wheel: A-6-7-8-9 (bits 12, 4, 5, 6, 7), nine high.
        if mask & 0x10F0 == 0x10F0 { 9 } else { 0 }
    } else {
        // Standard wheel: A-2-3-4-5 (bits 12, 0, 1, 2, 3), five high.
        if mask & 0x100F == 0x100F { 5 } else { 0 }
    }
}

fn build_straight_table(short_deck: bool) -> Vec<u8> {
    (0..MASK_COUNT)
        .map(|mask| straight_high_from_mask(mask as u16, short_deck))
        .collect()
}

/// Packs the five highest ranks of a flush mask into 4-bit nibbles (20 bits).
fn build_top_five_table() -> Vec<u32> {
    let mut table = vec![0u32; MASK_COUNT];
    for (mask_index, slot) in table.iter_mut().enumerate() {
        let mask = mask_index as u16;
        if mask.count_ones() < 5 {
            continue;
        }
        let mut packed = 0u32;
        let mut count = 0u32;
        let mut value = 14u8;
        while value >= 2 && count < 5 {
            if mask & (1u16 << (value - 2)) != 0 {
                packed |= u32::from(value) << (count * 4);
                count += 1;
            }
            value -= 1;
        }
        *slot = packed;
    }
    table
}

fn pack_result(category: HandCategory, kickers: [u8; 5]) -> u32 {
    let mut packed = category as u32;
    let mut index = 0usize;
    while index < 5 {
        packed |= u32::from(kickers[index]) << (4 + index * 4);
        index += 1;
    }
    packed
}

/// Decodes a packed table result into a category and five padded kicker values.
pub(super) fn unpack_result(packed: u32) -> (HandCategory, [u8; 5]) {
    let category = HandCategory::ALL[(packed & 0xF) as usize];
    let mut kickers = [0u8; 5];
    let mut index = 0usize;
    while index < 5 {
        kickers[index] = ((packed >> (4 + index * 4)) & 0xF) as u8;
        index += 1;
    }
    (category, kickers)
}

/// Variant-independent best-five result for a non-flush seven-card rank
/// histogram. Identical logic to the reference evaluator's non-flush branches.
fn rank_counts_result(counts: &[u8; RANK_COUNT], short_deck: bool) -> (HandCategory, [u8; 5]) {
    let mut mask = 0u16;
    let mut quads = 0u8;
    let mut trips = [0u8; 3];
    let mut trips_len = 0usize;
    let mut pairs = [0u8; 4];
    let mut pairs_len = 0usize;
    for offset in (0..RANK_COUNT).rev() {
        let count = counts[offset];
        if count == 0 {
            continue;
        }
        let value = offset as u8 + 2;
        mask |= 1u16 << offset;
        if count == 4 {
            quads = value;
        }
        if count >= 3 {
            trips[trips_len] = value;
            trips_len += 1;
        }
        if count >= 2 {
            pairs[pairs_len] = value;
            pairs_len += 1;
        }
    }

    if quads != 0 {
        let mut kicker = 0u8;
        for offset in (0..RANK_COUNT).rev() {
            let value = offset as u8 + 2;
            if value != quads && counts[offset] > 0 {
                kicker = value;
                break;
            }
        }
        return (HandCategory::FourOfAKind, [quads, kicker, 0, 0, 0]);
    }

    if trips_len > 0 {
        let trip = trips[0];
        let mut pair = 0u8;
        for &candidate in &pairs[..pairs_len] {
            if candidate != trip {
                pair = candidate;
                break;
            }
        }
        if pair != 0 {
            return (HandCategory::FullHouse, [trip, pair, 0, 0, 0]);
        }
    }

    let high = straight_high_from_mask(mask, short_deck);
    if high != 0 {
        return (HandCategory::Straight, [high, 0, 0, 0, 0]);
    }

    if trips_len > 0 {
        let trip = trips[0];
        let mut values = [trip, 0, 0, 0, 0];
        let mut len = 1usize;
        for offset in (0..RANK_COUNT).rev() {
            let value = offset as u8 + 2;
            if value != trip && counts[offset] > 0 {
                values[len] = value;
                len += 1;
                if len == 3 {
                    break;
                }
            }
        }
        return (HandCategory::ThreeOfAKind, values);
    }

    if pairs_len >= 2 {
        let first = pairs[0];
        let second = pairs[1];
        let mut kicker = 0u8;
        for offset in (0..RANK_COUNT).rev() {
            let value = offset as u8 + 2;
            if counts[offset] > 0 && value != first && value != second {
                kicker = value;
                break;
            }
        }
        return (HandCategory::TwoPair, [first, second, kicker, 0, 0]);
    }

    if pairs_len == 1 {
        let pair = pairs[0];
        let mut values = [pair, 0, 0, 0, 0];
        let mut len = 1usize;
        for offset in (0..RANK_COUNT).rev() {
            let value = offset as u8 + 2;
            if value != pair && counts[offset] > 0 {
                values[len] = value;
                len += 1;
                if len == 4 {
                    break;
                }
            }
        }
        return (HandCategory::Pair, values);
    }

    let mut values = [0u8; 5];
    let mut len = 0usize;
    for offset in (0..RANK_COUNT).rev() {
        if counts[offset] > 0 {
            values[len] = offset as u8 + 2;
            len += 1;
            if len == 5 {
                break;
            }
        }
    }
    (HandCategory::HighCard, values)
}

struct Pattern {
    key: u64,
    standard: u32,
    short_deck: u32,
}

fn enumerate_patterns(
    rank: usize,
    remaining: u8,
    counts: &mut [u8; RANK_COUNT],
    out: &mut Vec<Pattern>,
) {
    if rank == RANK_COUNT {
        if remaining == 0 {
            let mut key = 0u64;
            for (offset, &count) in counts.iter().enumerate() {
                key |= u64::from(count) << (offset * 4);
            }
            let (standard_category, standard_kickers) = rank_counts_result(counts, false);
            let (short_category, short_kickers) = rank_counts_result(counts, true);
            out.push(Pattern {
                key,
                standard: pack_result(standard_category, standard_kickers),
                short_deck: pack_result(short_category, short_kickers),
            });
        }
        return;
    }
    let slots_left = ((RANK_COUNT - rank - 1) as u8) * 4;
    let max_count = remaining.min(4);
    let mut count = 0u8;
    while count <= max_count {
        let rest = remaining - count;
        if rest <= slots_left {
            counts[rank] = count;
            enumerate_patterns(rank + 1, rest, counts, out);
        }
        count += 1;
    }
    counts[rank] = 0;
}

fn collect_patterns() -> Vec<Pattern> {
    let mut counts = [0u8; RANK_COUNT];
    let mut patterns = Vec::with_capacity(73_775);
    for size in 5..=7 {
        enumerate_patterns(0, size, &mut counts, &mut patterns);
    }
    patterns
}

const HASH_MULTIPLIERS: [u64; 8] = [
    0x9E37_79B9_7F4A_7C15,
    0xD6E8_FEB8_6659_FD93,
    0xA076_1D64_78BD_642F,
    0xE703_7ED1_A0B4_28DB,
    0x8EB4_4341_6C3D_9E2F,
    0xC2B2_AE3D_27D4_EB4F,
    0x1656_67B1_9E37_79F9,
    0x2545_F491_4F6C_DD1D,
];

fn bucket_index(key: u64, hash_mul: u64) -> usize {
    ((key.wrapping_mul(hash_mul)) >> HASH_SHIFT) as usize
}

fn choose_hash(patterns: &[Pattern]) -> u64 {
    let mut best = HASH_MULTIPLIERS[0];
    let mut best_max = usize::MAX;
    for &candidate in &HASH_MULTIPLIERS {
        let mut sizes = vec![0u32; BUCKET_COUNT];
        for pattern in patterns {
            sizes[bucket_index(pattern.key, candidate)] += 1;
        }
        let max = sizes.iter().copied().max().unwrap_or(0) as usize;
        if max < best_max {
            best_max = max;
            best = candidate;
        }
    }
    best
}

/// Immutable lookup tables for the fast seven-card path.
pub(super) struct Tables {
    bucket_start: Vec<u32>,
    keys: Vec<u64>,
    results_standard: Vec<u32>,
    results_short_deck: Vec<u32>,
    straight_standard: Vec<u8>,
    straight_short_deck: Vec<u8>,
    top_five: Vec<u32>,
    hash_mul: u64,
}

impl Tables {
    /// Looks up the packed non-flush result for a rank histogram key.
    pub(super) fn lookup(&self, key: u64, short_deck: bool) -> Option<u32> {
        let bucket = bucket_index(key, self.hash_mul);
        let start = self.bucket_start[bucket] as usize;
        let end = self.bucket_start[bucket + 1] as usize;
        let mut index = start;
        while index < end {
            if self.keys[index] == key {
                return Some(if short_deck {
                    self.results_short_deck[index]
                } else {
                    self.results_standard[index]
                });
            }
            index += 1;
        }
        None
    }

    /// Straight high card for a 13-bit rank mask (flush or non-flush).
    pub(super) fn straight_high(&self, mask: u16, short_deck: bool) -> u8 {
        let index = usize::from(mask);
        if short_deck {
            self.straight_short_deck[index]
        } else {
            self.straight_standard[index]
        }
    }

    /// Five highest ranks of a flush mask packed into 4-bit nibbles.
    pub(super) fn top_five(&self, mask: u16) -> u32 {
        self.top_five[usize::from(mask)]
    }
}

static TABLES: OnceLock<Tables> = OnceLock::new();

/// Returns the lazily initialized lookup tables.
pub(super) fn tables() -> &'static Tables {
    TABLES.get_or_init(build_tables)
}

fn build_tables() -> Tables {
    let straight_standard = build_straight_table(false);
    let straight_short_deck = build_straight_table(true);
    let top_five = build_top_five_table();

    let patterns = collect_patterns();
    debug_assert_eq!(
        patterns.len(),
        73_775,
        "five/six/seven-card rank pattern count"
    );
    let hash_mul = choose_hash(&patterns);

    let mut bucket_sizes = vec![0u32; BUCKET_COUNT];
    for pattern in &patterns {
        bucket_sizes[bucket_index(pattern.key, hash_mul)] += 1;
    }
    let mut bucket_start = vec![0u32; BUCKET_COUNT + 1];
    let mut total = 0u32;
    for index in 0..BUCKET_COUNT {
        bucket_start[index] = total;
        total += bucket_sizes[index];
    }
    bucket_start[BUCKET_COUNT] = total;

    let mut cursor: Vec<u32> = bucket_start[..BUCKET_COUNT].to_vec();
    let mut keys = vec![0u64; patterns.len()];
    let mut results_standard = vec![0u32; patterns.len()];
    let mut results_short_deck = vec![0u32; patterns.len()];
    for pattern in patterns {
        let bucket = bucket_index(pattern.key, hash_mul);
        let position = cursor[bucket] as usize;
        cursor[bucket] += 1;
        keys[position] = pattern.key;
        results_standard[position] = pattern.standard;
        results_short_deck[position] = pattern.short_deck;
    }

    Tables {
        bucket_start,
        keys,
        results_standard,
        results_short_deck,
        straight_standard,
        straight_short_deck,
        top_five,
        hash_mul,
    }
}

/// Rank histogram keys of every legal seven-card rank pattern (test helper).
#[cfg(test)]
pub(super) fn pattern_keys() -> Vec<u64> {
    collect_patterns()
        .into_iter()
        .map(|pattern| pattern.key)
        .collect()
}
