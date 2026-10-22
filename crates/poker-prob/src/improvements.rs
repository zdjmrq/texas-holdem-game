//! Outs / hand-improvement analysis for the next street.
//!
//! Port of `findImprovements()` from `js/probability-core.js`.

use std::collections::BTreeMap;

use poker_core::{Card, Deck, Variant, evaluate};

/// Character of an improving card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quality {
    /// The card raises the hand category (for example pair to trips).
    Strong,
    /// The card only strengthens the same category through kickers.
    Thin,
}

impl Quality {
    /// Lower-case name used by the JS implementation and the golden corpus.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Quality::Strong => "strong",
            Quality::Thin => "thin",
        }
    }
}

/// One card that improves the player's hand on the next street.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImprovementCard {
    /// Rank label as emitted by the JS evaluator: "2".."10", "J", "Q",
    /// "K", "A".
    pub rank: &'static str,
    /// Full suit name: "spades", "hearts", "diamonds" or "clubs".
    pub suit: &'static str,
    /// Numeric rank value 2..=14.
    pub value: u8,
    /// Strong or thin improvement.
    pub quality: Quality,
}

/// Every improving card that results in one particular hand type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImprovementGroup {
    /// Improving cards in canonical deck order (suit-major, ranks ascending).
    pub cards: Vec<ImprovementCard>,
    /// Total number of outs in this group.
    pub count: u32,
    /// Number of strong outs in this group.
    pub strong_count: u32,
    /// Number of thin outs in this group.
    pub thin_count: u32,
}

/// Outs report for the next street.
///
/// Mirrors the JS `findImprovements()` return value. `by_hand_type` is keyed by
/// the resulting hand's Chinese name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Improvements {
    /// Total number of outs (`strong_outs + thin_outs`).
    pub total_outs: u32,
    /// Outs that raise the hand category.
    pub strong_outs: u32,
    /// Outs that only strengthen the current category through kickers.
    pub thin_outs: u32,
    /// Outs grouped by resulting hand type.
    pub by_hand_type: BTreeMap<String, ImprovementGroup>,
    /// Human-readable explanation, identical to the JS note string.
    pub note: &'static str,
}

const HIDDEN_NOTE: &str = "翻牌后才显示牌型改善牌";
const VISIBLE_NOTE: &str = "强提升会提高牌型等级；踢脚改善只增强同一牌型，均不代表必胜";

fn hidden() -> Improvements {
    Improvements {
        total_outs: 0,
        strong_outs: 0,
        thin_outs: 0,
        by_hand_type: BTreeMap::new(),
        note: HIDDEN_NOTE,
    }
}

/// Finds the cards that improve the player's hand on the next street.
///
/// Mirrors `findImprovements(playerCards, communityCards, isShortDeck)` in
/// `js/probability-core.js`:
///
/// * only flop and turn boards (3 or 4 community cards) are analysed; every
///   other board length returns the JS early-return value;
/// * a card is an out only when it strictly increases the evaluator score;
/// * same-category improvements are dropped unless the hand is at least a pair
///   and the score gain is at least `1e6` (that is, a kicker moved by at least
///   one rank); ordinary high-card movement is never reported;
/// * an out is `strong` when the hand category (variant rank index) rises and
///   `thin` otherwise.
///
/// # Panics
///
/// Panics when the total number of supplied cards exceeds seven.
pub fn find_improvements(player: &[Card], board: &[Card], variant: Variant) -> Improvements {
    if board.len() < 3 || board.len() >= 5 {
        return hidden();
    }

    let mut known_mask = 0u64;
    for card in player.iter().chain(board) {
        known_mask |= card.bit();
    }
    let canonical_deck = Deck::new(variant);
    let deck: Vec<Card> = canonical_deck
        .cards()
        .iter()
        .copied()
        .filter(|card| known_mask & card.bit() == 0)
        .collect();

    let mut hand: Vec<Card> = Vec::with_capacity(7);
    hand.extend_from_slice(player);
    hand.extend_from_slice(board);
    let card_count = hand.len();
    assert!(
        card_count <= 7,
        "find_improvements supports at most seven cards, got {card_count}"
    );
    let before = evaluate(&hand, variant);

    let mut by_hand_type: BTreeMap<String, ImprovementGroup> = BTreeMap::new();
    let mut strong_outs = 0u32;
    let mut thin_outs = 0u32;
    for &card in &deck {
        hand.push(card);
        let after = evaluate(&hand, variant);
        hand.pop();
        if after.score() <= before.score() {
            continue;
        }
        // Do not call ordinary high-card movement an "out". Same-category
        // changes are only retained for made hands and meaningful kickers.
        if after.rank_index() == before.rank_index()
            && (before.rank_index() == 0 || after.score() - before.score() < 1_000_000)
        {
            continue;
        }
        let quality = if after.rank_index() > before.rank_index() {
            Quality::Strong
        } else {
            Quality::Thin
        };
        let group = by_hand_type
            .entry(after.name().to_string())
            .or_insert_with(|| ImprovementGroup {
                cards: Vec::new(),
                count: 0,
                strong_count: 0,
                thin_count: 0,
            });
        group.cards.push(ImprovementCard {
            rank: card.rank().as_str(),
            suit: card.suit().as_str(),
            value: card.rank_value(),
            quality,
        });
        group.count += 1;
        match quality {
            Quality::Strong => {
                strong_outs += 1;
                group.strong_count += 1;
            }
            Quality::Thin => {
                thin_outs += 1;
                group.thin_count += 1;
            }
        }
    }

    Improvements {
        total_outs: strong_outs + thin_outs,
        strong_outs,
        thin_outs,
        by_hand_type,
        note: VISIBLE_NOTE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use poker_core::Variant;

    fn cards(text: &str) -> Vec<Card> {
        text.split_whitespace()
            .map(|token| token.parse().expect("card"))
            .collect()
    }

    fn find(player: &str, board: &str, variant: Variant) -> Improvements {
        find_improvements(&cards(player), &cards(board), variant)
    }

    #[test]
    fn hidden_before_the_flop_and_on_the_river() {
        for board in ["", "Qs 2s 7d 9h 3c"] {
            let improvements = find("As Ks", board, Variant::Standard);
            assert_eq!(improvements.total_outs, 0);
            assert_eq!(improvements.strong_outs, 0);
            assert_eq!(improvements.thin_outs, 0);
            assert!(improvements.by_hand_type.is_empty());
            assert_eq!(improvements.note, "翻牌后才显示牌型改善牌");
        }
    }

    #[test]
    fn flush_draw_counts_spade_outs_in_deck_order() {
        let improvements = find("As Ks", "Qs 2s 7d", Variant::Standard);
        assert_eq!(improvements.strong_outs, 23);
        assert_eq!(improvements.thin_outs, 0);
        assert_eq!(improvements.total_outs, 23);
        assert_eq!(improvements.note, VISIBLE_NOTE);
        assert!(!improvements.by_hand_type.contains_key("高牌"));
        let flush = improvements.by_hand_type.get("同花").expect("flush outs");
        assert_eq!(flush.count, 9);
        assert_eq!(flush.strong_count, 9);
        assert_eq!(flush.thin_count, 0);
        assert_eq!(flush.cards.len(), 9);
        assert_eq!(flush.cards[0].rank, "3");
        assert_eq!(flush.cards[0].suit, "spades");
        assert_eq!(flush.cards[0].quality, Quality::Strong);
        assert_eq!(flush.cards[8].rank, "J");
    }

    #[test]
    fn trips_keep_only_meaningful_kicker_improvements() {
        let improvements = find("As Ah", "Ad 5c 2s", Variant::Standard);
        assert_eq!(improvements.total_outs, 39);
        assert_eq!(improvements.strong_outs, 7);
        assert_eq!(improvements.thin_outs, 32);
        let full_house = improvements.by_hand_type.get("葫芦").expect("boat outs");
        assert_eq!(full_house.count, 6);
        assert_eq!(full_house.strong_count, 6);
        let trips = improvements.by_hand_type.get("三条").expect("trip kickers");
        assert_eq!(trips.count, 32);
        assert_eq!(trips.strong_count, 0);
        assert_eq!(trips.thin_count, 32);
        let quads = improvements.by_hand_type.get("四条").expect("quad out");
        assert_eq!(quads.count, 1);
    }

    #[test]
    fn short_deck_flush_draw_uses_the_short_deck() {
        let improvements = find("As Ks", "Qs 8s 7d", Variant::ShortDeck);
        assert_eq!(improvements.strong_outs, 19);
        assert_eq!(improvements.total_outs, 19);
        let flush = improvements.by_hand_type.get("同花").expect("flush outs");
        assert_eq!(flush.count, 5);
        assert_eq!(flush.cards[0].rank, "6");
        assert_eq!(flush.cards[4].rank, "J");
    }
}
