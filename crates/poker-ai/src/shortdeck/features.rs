//! Short-deck helpers shared by the stable core: position categories, board
//! features and the public-action likelihood model.
//!
//! The board texture / combo feature functions are copied verbatim from
//! `js/ai-shortdeck-stable.js` (`stable:544-636`); they are identical to the
//! standard core and reuse the shared math module. The position category and
//! public-action likelihood are the short-deck variants from
//! `stable:107-117` and `stable:229-244`.

use poker_core::Variant;

use crate::input::DecisionContext;
use crate::math::{BoardComboFeatures, Position, board_combo_features, starting_hand_strength};

use super::types::PublicActionEvent;

/// Short-deck `positionCategory` (`js/ai-shortdeck-stable.js:107-117`).
///
/// Differences from the standard core: a two-player button is late position
/// (not a blind), the late-position distance uses `count - 2`, and middle
/// position is `distance <= max(1, floor(count * 0.32))`.
#[must_use]
pub(crate) fn position_category(context: &DecisionContext) -> Position {
    let count = context.player_count.max(2);
    let distance = context.position_from_button.max(0) as usize;
    if count == 2 && distance == 0 {
        return Position::Late;
    }
    if context.is_small_blind || context.is_big_blind {
        return Position::Blind;
    }
    if distance == 0 || distance >= count.saturating_sub(2) {
        return Position::Late;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let middle_limit = ((count as f64) * 0.32).floor() as usize;
    if distance <= middle_limit.max(1) {
        Position::Middle
    } else {
        Position::Early
    }
}

/// JS `analyzeTexture` for the short-deck core (identical to the standard
/// helper; kept as a named wrapper so the port is easy to audit).
#[must_use]
pub(crate) fn analyze_texture(
    board: &[poker_core::Card],
    hole_cards: &[poker_core::Card],
    variant: Variant,
) -> crate::math::Texture {
    crate::math::analyze_texture(board, hole_cards, variant)
}

/// JS `boardComboFeatures` (`stable:177-227`).
#[must_use]
pub(crate) fn combo_board_features(
    c1: poker_core::Card,
    c2: poker_core::Card,
    board: &[poker_core::Card],
    variant: Variant,
) -> BoardComboFeatures {
    board_combo_features(c1, c2, board, variant)
}

/// JS `startingHandStrength` (`stable:119-148`).
#[must_use]
pub(crate) fn hand_strength(
    c1: poker_core::Card,
    c2: poker_core::Card,
    variant: Variant,
    position: Position,
) -> f64 {
    starting_hand_strength(c1, c2, variant, position)
}

/// JS `publicActionLikelihood` (`stable:229-244`).
///
/// The reference core feeds the flattened belief-history record (not the raw
/// action metadata) into this function, so the argument type is
/// [PublicActionEvent].
#[must_use]
pub(crate) fn public_action_likelihood(
    features: &BoardComboFeatures,
    event: &PublicActionEvent,
    aggression: f64,
) -> f64 {
    let action = event.action.as_str();
    let value = features.value;
    let draw = features.draw;
    if action == "check" {
        return if value > 0.85 {
            0.65 + (1.0 - aggression) * 0.35
        } else {
            1.20 - value * 0.30 + draw * 0.18
        };
    }
    if action.contains("call") {
        let price = event.to_call / (event.pot + event.to_call).max(1.0);
        let draw_scale = if price > 0.30 { 0.40 } else { 0.85 };
        let mut result = 0.09 + value * 1.20 + draw * draw_scale;
        if event.fraction > 0.8 && value < 0.5 {
            result -= 0.18;
        }
        return result.max(0.06);
    }
    if action.contains("raise") || action == "bet" || action == "allin" {
        let pressure = if event.fraction > 0.85 { 1.25 } else { 1.0 };
        let bluff = (0.035 + features.nut_blocker * 0.18) * (0.6 + aggression);
        return (value.powf(3.2) * 3.0 * pressure + draw * (0.55 + aggression) + bluff).max(0.035);
    }
    1.0
}
