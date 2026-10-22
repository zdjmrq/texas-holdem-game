//! Variant-neutral betting rules ported from `js/game-rules-core.js`:
//! raise grid, side pots, uncalled refunds, action metadata and decision ids.

use poker_core::Variant;

use crate::player::Player;

/// Result of aligning a raise range to the wager grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RaiseBounds {
    /// Smallest grid-aligned raise total.
    pub min: u64,
    /// Largest grid-aligned total.
    pub max: u64,
    /// Grid unit.
    pub unit: u64,
}

/// Only voluntary raises use this grid; calls and all-ins pay exact chip counts.
#[must_use]
pub fn raise_bounds(minimum: u64, maximum: u64, unit: u64) -> RaiseBounds {
    let unit = unit.max(1);
    RaiseBounds {
        min: minimum.div_ceil(unit) * unit,
        max: (maximum / unit) * unit,
        unit,
    }
}

/// Snaps an arbitrary amount onto the legal raise grid.
///
/// Mirrors the original JS `snapRaise`: rounds to the nearest grid step, then
/// clamps into [min, max]; returns None when the range is empty or the amount
/// is not finite.
#[must_use]
pub fn snap_raise(amount: f64, minimum: u64, maximum: u64, unit: u64) -> Option<u64> {
    let bounds = raise_bounds(minimum, maximum, unit);
    if bounds.min > bounds.max || !amount.is_finite() {
        return None;
    }
    let step = bounds.unit as f64;
    let snapped = (amount / step).round() * step;
    let lower = bounds.min as f64;
    let upper = bounds.max as f64;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Some(snapped.clamp(lower, upper) as u64)
}

/// Builds the original JS decision identity string
/// (`variant:phase:actionOrdinal:seatId`).
#[must_use]
pub fn decision_identity(
    variant: Variant,
    phase: &str,
    action_ordinal: u32,
    seat_id: usize,
) -> String {
    let variant = if variant.is_short_deck() {
        "shortdeck"
    } else {
        "standard"
    };
    let phase = if phase.is_empty() { "preflop" } else { phase };
    format!("{variant}:{phase}:{action_ordinal}:{seat_id}")
}

/// One side pot and the seats eligible to win it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SidePot {
    /// Chips in the pot.
    pub amount: u64,
    /// Eligible seat indices in seat order.
    pub eligible: Vec<usize>,
}

/// Splits the pot into main/side pots from every contribution, including chips
/// from players who have folded. Port of the original `calculateSidePots`.
#[must_use]
pub fn calculate_side_pots(
    players: &[Player],
    total_pot: u64,
    eligible_players: &[usize],
) -> Vec<SidePot> {
    let eligible_set: std::collections::BTreeSet<usize> =
        eligible_players.iter().copied().collect();
    if eligible_set.len() <= 1 {
        return vec![SidePot {
            amount: total_pot,
            eligible: eligible_set.into_iter().collect(),
        }];
    }
    let mut levels: Vec<u64> = players
        .iter()
        .map(|player| player.chips_in_pot)
        .filter(|amount| *amount > 0)
        .collect();
    levels.sort_unstable();
    levels.dedup();
    let mut pots: Vec<SidePot> = Vec::new();
    let mut previous = 0u64;
    for level in levels {
        let contributors: Vec<usize> = players
            .iter()
            .enumerate()
            .filter(|(_, player)| player.chips_in_pot >= level)
            .map(|(index, _)| index)
            .collect();
        let eligible: Vec<usize> = contributors
            .iter()
            .copied()
            .filter(|index| eligible_set.contains(index))
            .collect();
        let amount = (level - previous) * contributors.len() as u64;
        if amount > 0 && !eligible.is_empty() {
            pots.push(SidePot { amount, eligible });
        }
        previous = level;
    }
    let accounted: u64 = pots.iter().map(|pot| pot.amount).sum();
    let remainder = total_pot.saturating_sub(accounted);
    if remainder > 0
        && let Some(first) = pots.first_mut()
    {
        first.amount += remainder;
    }
    pots
}

/// The unique unmatched top wager of a betting round.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UncalledRefund {
    /// Seat index that gets the refund.
    pub index: usize,
    /// Chips returned.
    pub refund: u64,
}

/// Finds the unique unmatched top wager of a betting round.
/// Port of the original `findUncalledRefund`.
#[must_use]
pub fn find_uncalled_refund(round_bets: &[u64]) -> Option<UncalledRefund> {
    if round_bets.is_empty() {
        return None;
    }
    let mut entries: Vec<(usize, u64)> = round_bets.iter().copied().enumerate().collect();
    // Descending stable sort by amount, matching JS Array.sort stability.
    entries.sort_by_key(|entry| std::cmp::Reverse(entry.1));
    let top = entries[0];
    let second = entries.get(1).map_or(0, |entry| entry.1);
    if top.1 == second {
        return None;
    }
    Some(UncalledRefund {
        index: top.0,
        refund: top.1 - second,
    })
}

/// Action metadata passed to opponent-modeling code (port of `buildActionMeta`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActionMeta {
    /// Amount needed to call before the action.
    pub call_cost: u64,
    /// Alias of call_cost (JS keeps both names).
    pub to_call_before: u64,
    /// Chips added by this action.
    pub additional_paid: u64,
    /// Alias of additional_paid.
    pub amount: u64,
    /// Total round bet after the action.
    pub raise_to: u64,
    /// Table current bet before the action.
    pub current_bet_before: u64,
    /// Pot before the action.
    pub pot_before: u64,
    /// Pot after the action.
    pub pot_after: u64,
    /// Minimum bet unit.
    pub minimum_bet: u64,
    /// Whether the action increased the current bet.
    pub aggressive: bool,
    /// Bet size divided by max(1, minimum bet, pot before).
    pub bet_fraction: f64,
    /// Flop c-bet opportunity flag.
    pub is_cbet_opportunity: bool,
    /// Facing a flop continuation bet.
    pub faced_cbet: bool,
    /// Facing a pre-flop 3-bet or later.
    pub faced_3bet: bool,
    /// Pre-flop raise count before this action.
    pub preflop_raise_count_before: u32,
}

/// Inputs for [`build_action_meta`].
#[derive(Clone, Copy, Debug, Default)]
pub struct ActionMetaInput {
    /// Amount needed to call.
    pub call_cost: u64,
    /// Chips added by the action.
    pub additional_paid: u64,
    /// Total round bet after the action.
    pub raise_to: u64,
    /// Current bet before the action.
    pub current_bet_before: u64,
    /// Pot before the action.
    pub pot_before: u64,
    /// Pot after the action.
    pub pot_after: u64,
    /// Minimum bet unit.
    pub minimum_bet: u64,
    /// Whether the action was aggressive.
    pub aggressive: bool,
    /// Flop c-bet opportunity flag.
    pub is_cbet_opportunity: bool,
    /// Facing a flop continuation bet.
    pub faced_cbet: bool,
    /// Facing a pre-flop 3-bet or later.
    pub faced_3bet: bool,
    /// Pre-flop raise count before this action.
    pub preflop_raise_count_before: u32,
}

/// Builds action metadata exactly like the original `buildActionMeta`.
#[must_use]
pub fn build_action_meta(input: ActionMetaInput) -> ActionMeta {
    let wager_size = if input.aggressive {
        input.raise_to.saturating_sub(input.current_bet_before)
    } else {
        input.additional_paid
    };
    let denominator = input.minimum_bet.max(1).max(input.pot_before);
    ActionMeta {
        call_cost: input.call_cost,
        to_call_before: input.call_cost,
        additional_paid: input.additional_paid,
        amount: input.additional_paid,
        raise_to: input.raise_to,
        current_bet_before: input.current_bet_before,
        pot_before: input.pot_before,
        pot_after: input.pot_after,
        minimum_bet: input.minimum_bet,
        aggressive: input.aggressive,
        bet_fraction: wager_size as f64 / denominator as f64,
        is_cbet_opportunity: input.is_cbet_opportunity,
        faced_cbet: input.faced_cbet,
        faced_3bet: input.faced_3bet,
        preflop_raise_count_before: input.preflop_raise_count_before,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::player::Player;

    fn seats(chips: &[u64]) -> Vec<Player> {
        chips
            .iter()
            .enumerate()
            .map(|(index, chips)| {
                let mut player = Player::new(format!("p{index}"), index, format!("P{index}"), 0);
                player.chips_in_pot = *chips;
                player
            })
            .collect()
    }

    #[test]
    fn raise_grid_rounds_up_min_and_down_max() {
        let bounds = raise_bounds(105, 990, 100);
        assert_eq!(
            bounds,
            RaiseBounds {
                min: 200,
                max: 900,
                unit: 100
            }
        );
    }

    #[test]
    fn snap_raise_lands_on_the_grid_and_clamps() {
        assert_eq!(snap_raise(1234.0, 200, 2000, 100), Some(1200));
        assert_eq!(snap_raise(123.0, 200, 2000, 100), Some(200));
        assert_eq!(snap_raise(5000.0, 200, 2000, 100), Some(2000));
        assert_eq!(snap_raise(f64::NAN, 200, 2000, 100), None);
    }

    #[test]
    fn side_pots_include_folded_contributions() {
        let players = seats(&[200, 500, 500, 100]);
        let pots = calculate_side_pots(&players, 1300, &[1, 2]);
        let total: u64 = pots.iter().map(|pot| pot.amount).sum();
        assert_eq!(total, 1300);
        // Level 100 includes all four players (4*100), level 200 adds three
        // players (+3*100), level 500 adds the two deep stacks (+2*300).
        assert_eq!(pots.len(), 3);
        assert_eq!(pots[0].amount, 400);
        assert_eq!(pots[1].amount, 300);
        assert_eq!(pots[2].amount, 600);
        assert_eq!(pots[2].eligible, vec![1, 2]);
    }

    #[test]
    fn side_pots_single_eligible_returns_whole_pot() {
        let players = seats(&[100, 300]);
        let pots = calculate_side_pots(&players, 400, &[1]);
        assert_eq!(
            pots,
            vec![SidePot {
                amount: 400,
                eligible: vec![1]
            }]
        );
    }

    #[test]
    fn uncalled_refund_is_the_unique_top_wager() {
        assert_eq!(
            find_uncalled_refund(&[500, 200, 200]),
            Some(UncalledRefund {
                index: 0,
                refund: 300
            })
        );
        assert_eq!(find_uncalled_refund(&[200, 200, 0]), None);
        assert_eq!(find_uncalled_refund(&[0, 0, 0]), None);
        assert_eq!(find_uncalled_refund(&[]), None);
        assert_eq!(
            find_uncalled_refund(&[0, 100]),
            Some(UncalledRefund {
                index: 1,
                refund: 100
            })
        );
    }

    #[test]
    fn decision_identity_matches_js_format() {
        assert_eq!(
            decision_identity(Variant::Standard, "flop", 3, 2),
            "standard:flop:3:2"
        );
        assert_eq!(
            decision_identity(Variant::ShortDeck, "", 0, 1),
            "shortdeck:preflop:0:1"
        );
    }

    #[test]
    fn action_meta_matches_formula() {
        let meta = build_action_meta(ActionMetaInput {
            call_cost: 50,
            additional_paid: 150,
            raise_to: 200,
            current_bet_before: 100,
            pot_before: 300,
            pot_after: 500,
            minimum_bet: 100,
            aggressive: true,
            ..ActionMetaInput::default()
        });
        assert_eq!(meta.amount, 150);
        assert_eq!(meta.raise_to, 200);
        assert!((meta.bet_fraction - 100.0 / 300.0).abs() < f64::EPSILON);
    }
}
