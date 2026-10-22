//! Game variants: standard Texas hold'em (52 cards) and 6+ short deck (36 cards).

use crate::card::Rank;

/// The two supported game variants.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Variant {
    /// Standard Texas hold'em: 52 cards, ranks 2..=Ace, flush ranked below full house.
    Standard,
    /// 6+ short deck: 36 cards, ranks 6..=Ace, flush ranked above full house,
    /// A-6-7-8-9 wheel straight.
    ShortDeck,
}

impl Variant {
    /// Both variants in display order.
    pub const ALL: [Variant; 2] = [Variant::Standard, Variant::ShortDeck];

    /// Number of cards in the variant's deck.
    #[must_use]
    pub const fn deck_size(self) -> usize {
        match self {
            Variant::Standard => 52,
            Variant::ShortDeck => 36,
        }
    }

    /// True for the 36-card short deck.
    #[must_use]
    pub const fn is_short_deck(self) -> bool {
        matches!(self, Variant::ShortDeck)
    }

    /// Ranks present in the variant's deck, ascending (2..=Ace or 6..=Ace).
    #[must_use]
    pub const fn ranks(self) -> &'static [Rank] {
        match self {
            Variant::Standard => &STANDARD_RANKS,
            Variant::ShortDeck => &SHORT_DECK_RANKS,
        }
    }

    /// Chinese hand-type names indexed by the variant's internal rank index.
    ///
    /// The index order follows the original JS HAND_TYPE_NAMES arrays:
    /// short deck swaps flush and full house.
    #[must_use]
    pub const fn hand_type_names(self) -> &'static [&'static str; 10] {
        match self {
            Variant::Standard => &STANDARD_HAND_NAMES,
            Variant::ShortDeck => &SHORT_DECK_HAND_NAMES,
        }
    }
}

const STANDARD_RANKS: [Rank; 13] = [
    Rank::Two,
    Rank::Three,
    Rank::Four,
    Rank::Five,
    Rank::Six,
    Rank::Seven,
    Rank::Eight,
    Rank::Nine,
    Rank::Ten,
    Rank::Jack,
    Rank::Queen,
    Rank::King,
    Rank::Ace,
];

const SHORT_DECK_RANKS: [Rank; 9] = [
    Rank::Six,
    Rank::Seven,
    Rank::Eight,
    Rank::Nine,
    Rank::Ten,
    Rank::Jack,
    Rank::Queen,
    Rank::King,
    Rank::Ace,
];

const STANDARD_HAND_NAMES: [&str; 10] = [
    "高牌",
    "一对",
    "两对",
    "三条",
    "顺子",
    "同花",
    "葫芦",
    "四条",
    "同花顺",
    "皇家同花顺",
];

const SHORT_DECK_HAND_NAMES: [&str; 10] = [
    "高牌",
    "一对",
    "两对",
    "三条",
    "顺子",
    "葫芦",
    "同花",
    "四条",
    "同花顺",
    "皇家同花顺",
];
