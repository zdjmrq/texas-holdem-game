//! Shared AI math: position, starting-hand strength, range cutoffs, board
//! texture and public-action likelihood.
//!
//! Ports js/ai-core.js (docs/specs/ai-strategy.md sections 4 and 6.6).

use std::collections::HashSet;

use poker_core::{Card, Variant, evaluate};

use crate::input::Street;

/// Position class from the original AI (early/middle/late/blind).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Position {
    /// Early position.
    Early,
    /// Middle position.
    Middle,
    /// Late position / button.
    Late,
    /// Blinds.
    Blind,
}

impl Position {
    /// Original two-letter code.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Position::Early => "EP",
            Position::Middle => "MP",
            Position::Late => "LP",
            Position::Blind => "BL",
        }
    }
}

/// Standard positionCategory (js/ai-core.js:139-148).
#[must_use]
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn position_category(
    player_count: usize,
    position_from_button: i32,
    is_small_blind: bool,
    is_big_blind: bool,
) -> Position {
    let count = player_count.max(2);
    let distance = position_from_button.max(0) as usize;
    if count == 2 && distance == 0 {
        return Position::Late;
    }
    if is_small_blind || is_big_blind {
        return Position::Blind;
    }
    if distance == 0 || distance == count - 1 {
        return Position::Late;
    }
    let early_limit = 3 + (count.saturating_sub(3)).div_ceil(3).max(1);
    if distance < early_limit {
        Position::Early
    } else {
        Position::Middle
    }
}

/// Features of one concrete starting combination.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ComboFeatures {
    /// Higher rank value.
    pub high: u8,
    /// Lower rank value.
    pub low: u8,
    /// Pocket pair.
    pub pair: bool,
    /// Suited.
    pub suited: bool,
    /// Rank gap.
    pub gap: u8,
    /// Holds an ace.
    pub ace_blocker: bool,
    /// Holds a king.
    pub king_blocker: bool,
    /// Static starting strength.
    pub strength: f64,
}

/// JS startingHandStrength (js/ai-core.js:150-179).
#[must_use]
#[allow(clippy::cast_precision_loss)]
pub fn starting_hand_strength(c1: Card, c2: Card, variant: Variant, position: Position) -> f64 {
    let high = c1.rank_value().max(c2.rank_value());
    let low = c1.rank_value().min(c2.rank_value());
    let pair = high == low;
    let suited = c1.suit() == c2.suit();
    let gap = f64::from(high - low);
    let short_deck = variant.is_short_deck();
    let mut strength;
    if pair {
        let base = if short_deck { 6.0 } else { 2.0 };
        strength = 0.49 + (f64::from(high) - base) / (14.0 - base) * 0.46;
        if high >= 11 {
            strength += 0.035;
        }
    } else {
        strength = 0.08 + (f64::from(high) - 6.0) * 0.043 + (f64::from(low) - 7.0).max(0.0) * 0.025;
        if high == 14 {
            strength += 0.075;
        }
        if suited {
            strength += 0.075;
        }
        if gap == 1.0 {
            strength += 0.065;
        } else if gap == 2.0 {
            strength += 0.035;
        } else if gap >= 5.0 {
            strength -= 0.055;
        }
        if high >= 11 && low >= 10 {
            strength += 0.08;
        }
    }
    if short_deck {
        strength += if pair || suited { 0.025 } else { 0.0 };
        if !pair && low < 9 {
            strength -= 0.045;
        }
    }
    match position {
        Position::Late => strength += 0.055,
        Position::Early => strength -= 0.045,
        _ => {}
    }
    strength.clamp(0.02, 0.98)
}

/// JS comboFeatures (js/ai-core.js:181-194).
#[must_use]
pub fn combo_features(c1: Card, c2: Card, variant: Variant) -> ComboFeatures {
    let high = c1.rank_value().max(c2.rank_value());
    let low = c1.rank_value().min(c2.rank_value());
    ComboFeatures {
        high,
        low,
        pair: high == low,
        suited: c1.suit() == c2.suit(),
        gap: high - low,
        ace_blocker: high == 14,
        king_blocker: high == 13,
        strength: starting_hand_strength(c1, c2, variant, Position::Middle),
    }
}

/// JS startingRangeCutoff: strength at the (1 - width) quantile of all starting
/// combinations for the variant.
#[must_use]
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
pub fn starting_range_cutoff(width: f64, variant: Variant) -> f64 {
    static STANDARD: std::sync::OnceLock<Vec<f64>> = std::sync::OnceLock::new();
    static SHORTDECK: std::sync::OnceLock<Vec<f64>> = std::sync::OnceLock::new();
    let scores = match variant {
        Variant::Standard => STANDARD.get_or_init(|| sorted_starting_strengths(variant)),
        Variant::ShortDeck => SHORTDECK.get_or_init(|| sorted_starting_strengths(variant)),
    };
    let width = width.clamp(0.0, 1.0);
    let index = (((1.0 - width) * scores.len() as f64) as usize).min(scores.len() - 1);
    scores[index]
}

fn sorted_starting_strengths(variant: Variant) -> Vec<f64> {
    let deck = deck_cards(variant);
    let mut scores = Vec::with_capacity(deck.len() * (deck.len() - 1) / 2);
    for i in 0..deck.len() {
        for j in (i + 1)..deck.len() {
            scores.push(starting_hand_strength(
                deck[i],
                deck[j],
                variant,
                Position::Middle,
            ));
        }
    }
    scores.sort_by(f64::total_cmp);
    scores
}

/// Variant deck in original JS order (suit-major, rank ascending).
#[must_use]
pub fn deck_cards(variant: Variant) -> Vec<Card> {
    let mut cards = Vec::with_capacity(variant.deck_size());
    for suit in poker_core::Suit::ALL {
        for rank in variant.ranks() {
            cards.push(Card::new(suit, *rank));
        }
    }
    cards
}

/// Features of one concrete combination on the public board.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BoardComboFeatures {
    /// Made-hand value in [0.1, 0.995].
    pub value: f64,
    /// Draw strength in [0, 0.9].
    pub draw: f64,
    /// Flush draw.
    pub flush_draw: bool,
    /// Open-ended straight draw.
    pub open_ended: bool,
    /// Nut blocker weight in [0, 1].
    pub nut_blocker: f64,
    /// Evaluator score.
    pub score: u64,
    /// Evaluator variant rank index.
    pub rank: u8,
}

fn count_values(cards: &[Card]) -> Vec<(u8, u32)> {
    let mut out: Vec<(u8, u32)> = Vec::new();
    for card in cards {
        let value = card.rank_value();
        match out.iter_mut().find(|(existing, _)| *existing == value) {
            Some((_, count)) => *count += 1,
            None => out.push((value, 1)),
        }
    }
    out
}

fn count_suits(cards: &[Card]) -> Vec<(poker_core::Suit, u32)> {
    let mut out: Vec<(poker_core::Suit, u32)> = Vec::new();
    for card in cards {
        match out
            .iter_mut()
            .find(|(existing, _)| *existing == card.suit())
        {
            Some((_, count)) => *count += 1,
            None => out.push((card.suit(), 1)),
        }
    }
    out
}

/// JS boardComboFeatures (js/ai-core.js:207-258).
#[must_use]
#[allow(clippy::cast_precision_loss, clippy::too_many_lines)]
pub fn board_combo_features(
    c1: Card,
    c2: Card,
    board: &[Card],
    variant: Variant,
) -> BoardComboFeatures {
    let hole = [c1, c2];
    let mut cards: Vec<Card> = hole.to_vec();
    cards.extend_from_slice(board);
    let made = evaluate(&cards, variant);
    let ranks = count_values(&cards);
    let top = board
        .iter()
        .map(|card| card.rank_value())
        .max()
        .unwrap_or(0);
    let mut value = 0.10_f64;
    let name = made.name();
    if name == "一对" {
        let pair = ranks
            .iter()
            .find(|(_, count)| *count >= 2)
            .map(|(value, _)| *value)
            .unwrap_or(0);
        let owns_pair = hole.iter().any(|card| card.rank_value() == pair);
        value = if !owns_pair {
            0.21
        } else if pair > top {
            0.72
        } else if pair == top {
            0.62
        } else {
            0.38
        };
    } else if name == "两对" {
        value = 0.78;
    } else if name == "三条" {
        value = 0.88;
    } else if name == "顺子" {
        value = 0.92;
    } else if name == "同花" {
        value = 0.95;
    } else if name == "葫芦" {
        value = 0.975;
    } else if made.rank_index() >= 7 {
        value = 0.995;
    }
    if board.len() >= 3 && (name == "两对" || name == "三条") {
        let public_made = evaluate(board, variant);
        let board_values: HashSet<u8> = board.iter().map(|card| card.rank_value()).collect();
        if public_made.rank_index() == made.rank_index()
            && !hole
                .iter()
                .any(|card| board_values.contains(&card.rank_value()))
        {
            let max_hole = hole.iter().map(|card| card.rank_value()).max().unwrap_or(0);
            value = 0.20 + 0.20 * f64::from(max_hole) / 14.0;
        }
    }
    if board.len() == 5 && made.score() == evaluate(board, variant).score() {
        value = 0.25;
    }
    let suits = count_suits(&cards);
    let board_suits = count_suits(board);
    let mut flush_draw = false;
    let mut nut_blocker = 0.0_f64;
    for (suit, count) in &suits {
        if board.len() < 5 && *count == 4 && hole.iter().any(|card| card.suit() == *suit) {
            flush_draw = true;
        }
        if board_suits
            .iter()
            .any(|(board_suit, board_count)| board_suit == suit && *board_count >= 2)
        {
            if hole
                .iter()
                .any(|card| card.suit() == *suit && card.rank_value() == 14)
            {
                nut_blocker += 0.7;
            } else if hole
                .iter()
                .any(|card| card.suit() == *suit && card.rank_value() == 13)
            {
                nut_blocker += 0.25;
            }
        }
    }
    let mut values: HashSet<u8> = cards.iter().map(|card| card.rank_value()).collect();
    let mut board_values: HashSet<u8> = board.iter().map(|card| card.rank_value()).collect();
    let ace_low = if variant.is_short_deck() { 5 } else { 1 };
    if values.contains(&14) {
        values.insert(ace_low);
    }
    if board_values.contains(&14) {
        board_values.insert(ace_low);
    }
    let mut straight_out_ranks: HashSet<u8> = HashSet::new();
    if board.len() < 5 {
        for low in ace_low..=10 {
            let run = [low, low + 1, low + 2, low + 3, low + 4];
            let missing: Vec<u8> = run
                .iter()
                .copied()
                .filter(|rank| !values.contains(rank))
                .collect();
            if missing.len() == 1
                && run
                    .iter()
                    .any(|rank| values.contains(rank) && !board_values.contains(rank))
            {
                straight_out_ranks.insert(if missing[0] == ace_low {
                    14
                } else {
                    missing[0]
                });
            }
        }
    }
    let open_ended = straight_out_ranks.len() >= 2;
    let mut draw: f64 = if flush_draw { 0.58 } else { 0.0 };
    if open_ended {
        draw += 0.42;
    } else if !straight_out_ranks.is_empty() {
        draw += 0.24;
    }
    let draw = draw.min(0.90);
    BoardComboFeatures {
        value,
        draw,
        flush_draw,
        open_ended,
        nut_blocker: nut_blocker.min(1.0),
        score: made.score(),
        rank: made.rank_index(),
    }
}

/// JS publicActionLikelihood (js/ai-core.js:260-275).
#[must_use]
pub fn public_action_likelihood(
    features: &BoardComboFeatures,
    action: &str,
    to_call_before: f64,
    pot_before: f64,
    bet_fraction: f64,
    aggression: f64,
) -> f64 {
    let value = features.value;
    let draw = features.draw;
    let action = action.to_ascii_lowercase();
    if action == "check" {
        return if value > 0.85 {
            0.65 + (1.0 - aggression) * 0.35
        } else {
            1.20 - value * 0.30 + draw * 0.18
        };
    }
    if action.contains("call") {
        let price = to_call_before / (pot_before + to_call_before).max(1.0);
        let draw_scale = if price > 0.30 { 0.40 } else { 0.85 };
        let mut result = 0.09 + value * 1.20 + draw * draw_scale;
        if bet_fraction > 0.8 && value < 0.5 {
            result -= 0.18;
        }
        return result.max(0.06);
    }
    if action.contains("raise") || action == "bet" || action == "allin" {
        let pressure = if bet_fraction > 0.85 { 1.25 } else { 1.0 };
        let bluff = (0.035 + features.nut_blocker * 0.18) * (0.6 + aggression);
        return (value.powf(3.2) * 3.0 * pressure + draw * (0.55 + aggression) + bluff).max(0.035);
    }
    1.0
}

/// Public board texture summary (js/ai-core.js:598-647).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Texture {
    /// Board is paired.
    pub paired: bool,
    /// Three or more of a suit on the board.
    pub monotone: bool,
    /// Hero has a flush draw.
    pub has_flush_draw: bool,
    /// Hero has a straight draw.
    pub has_straight_draw: bool,
    /// Hero has an open-ended straight draw.
    pub open_ended: bool,
    /// Hero has a gutshot.
    pub gutshot: bool,
    /// Hero has a strong draw (flush draw or open-ended).
    pub has_strong_draw: bool,
    /// Nut blocker weight.
    pub nut_blocker: f64,
    /// Board wetness in [0, 1].
    pub wetness: f64,
    /// Highest board rank value.
    pub high_card: u8,
    /// Last card is a pressure card.
    pub pressure_card: bool,
}

/// JS analyzeTexture (js/ai-core.js:598-647).
#[must_use]
#[allow(clippy::cast_precision_loss, clippy::too_many_lines)]
pub fn analyze_texture(board: &[Card], hole: &[Card], variant: Variant) -> Texture {
    let board_ranks = count_values(board);
    let board_suits = count_suits(board);
    let paired = board_ranks.iter().any(|(_, count)| *count >= 2);
    let monotone = board_suits.iter().any(|(_, count)| *count >= 3);
    let features = if hole.len() == 2 {
        board_combo_features(hole[0], hole[1], board, variant)
    } else {
        BoardComboFeatures::default()
    };
    let has_flush_draw = features.flush_draw;
    let mut sorted: Vec<u8> = board_ranks.iter().map(|(value, _)| *value).collect();
    sorted.sort_unstable();
    let mut connectedness = 0.0;
    for pair in sorted.windows(2) {
        if pair[1] - pair[0] <= 2 {
            connectedness += 1.0;
        }
    }
    let mut wetness: f64 = if monotone {
        0.34
    } else if board_suits.iter().any(|(_, count)| *count == 2) {
        0.17
    } else {
        0.0
    };
    wetness += connectedness * 0.11 + if paired { 0.10 } else { 0.0 };
    let open_ended = features.open_ended;
    let gutshot = features.draw > 0.0 && !open_ended && !has_flush_draw;
    let last = board.last();
    Texture {
        paired,
        monotone,
        has_flush_draw,
        has_straight_draw: open_ended || gutshot,
        open_ended,
        gutshot,
        has_strong_draw: has_flush_draw || open_ended,
        nut_blocker: features.nut_blocker,
        wetness: wetness.clamp(0.0, 1.0),
        high_card: board
            .iter()
            .map(|card| card.rank_value())
            .max()
            .unwrap_or(0),
        pressure_card: last.is_some_and(|card| card.rank_value() >= 11 || paired || monotone),
    }
}

/// Street index helper used by sizing logic.
#[must_use]
pub const fn streets_remaining(street: Street) -> u32 {
    match street {
        Street::Preflop | Street::Flop => 3,
        Street::Turn => 2,
        Street::River => 1,
    }
}
