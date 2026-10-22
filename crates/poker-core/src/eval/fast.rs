//! Fast seven-card evaluator.
//!
//! Two-stage evaluation, proven bit-identical to the reference path:
//!
//! 1. Fold the seven cards into a packed rank histogram plus four 13-bit suit
//!    masks (one table lookup + add/or per card).
//! 2. If a suit has at least five cards, the best hand is always flush-family:
//!    with five flush cards and only two other cards a full house or four of a
//!    kind is impossible (trips would need two off-suit cards and the pair
//!    another two off-suit cards), so the flush cannot be beaten by a non-flush
//!    category. Straight flush / royal flush are detected with the flush mask,
//!    otherwise the five highest flush ranks are used.
//! 3. Without a flush, the best hand depends only on the rank histogram, so a
//!    single bucket-sorted table lookup returns the category and kickers.
//
//! ## Score assembly
//!
//! The score itself is assembled by the shared `eval::finish`, which keeps
//! `rank_index`, `score` and `name` bit-identical to the reference evaluator.

use crate::card::Card;
use crate::variant::Variant;

use super::tables::{self, CARD_COUNT, CARD_RANK_BIT, CARD_SUIT};
use super::{EvaluatedHand, HandCategory, MAX_EVALUATED_CARDS, finish};

/// Evaluates a hand of exactly seven cards. Callers must pass seven distinct
/// cards; invalid input is still handled correctly because a histogram without
/// a table entry falls back to the reference evaluator.
#[inline]
pub(super) fn evaluate_seven(cards: &[Card], variant: Variant) -> EvaluatedHand {
    debug_assert_eq!(cards.len(), MAX_EVALUATED_CARDS);
    let mut rank_key = 0u64;
    let mut suit_masks = [0u16; 4];
    for card in cards {
        let index = usize::from(card.index());
        rank_key += CARD_COUNT[index];
        suit_masks[usize::from(CARD_SUIT[index])] |= CARD_RANK_BIT[index];
    }

    let tables = tables::tables();
    let short_deck = variant.is_short_deck();

    let mut flush_mask = 0u16;
    for &mask in &suit_masks {
        if mask.count_ones() >= 5 {
            flush_mask = mask;
            break;
        }
    }

    if flush_mask != 0 {
        let high = tables.straight_high(flush_mask, short_deck);
        if high == 0 {
            let packed = tables.top_five(flush_mask);
            let mut kickers = [0u8; 5];
            for (index, slot) in kickers.iter_mut().enumerate() {
                *slot = ((packed >> (index * 4)) & 0xF) as u8;
            }
            return finish(variant, HandCategory::Flush, &kickers);
        }
        if high == 14 {
            return finish(variant, HandCategory::RoyalFlush, &[]);
        }
        return finish(variant, HandCategory::StraightFlush, &[high]);
    }

    match tables.lookup(rank_key, short_deck) {
        Some(packed) => {
            let (category, kickers) = tables::unpack_result(packed);
            finish(variant, category, &kickers)
        }
        // Only reachable for malformed hands (e.g. duplicate cards); the
        // reference path keeps the public behaviour well defined.
        None => super::reference::evaluate(cards, variant),
    }
}
