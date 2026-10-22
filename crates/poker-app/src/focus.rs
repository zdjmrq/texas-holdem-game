//! Logical keyboard focus chains (docs/specs/ui.md 4.4/4.7).
//!
//! egui 0.37 already moves focus with Tab/Shift+Tab in widget creation order
//! (Memory::interested_in_focus), so the chain is realised by building widgets
//! in the order defined here and by requesting focus on state transitions.
//! These pure order definitions are unit-tested and keep the spec reviewable.

use eframe::egui;

/// One focusable target in the logical Tab order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FocusTarget {
    // Home screen
    StartGame,
    OnlineGame,
    OpenSettings,
    VariantStandard,
    VariantShortDeck,
    Players,
    Stack,
    Blinds,
    PlayerName,
    AiDelay,
    // Table action bar
    Fold,
    Check,
    Call,
    Raise,
    AllIn,
    // Top bar toggles
    HideCards,
    Music,
    Analysis,
    TableSettings,
    // Settlement modal
    ModalNextHand,
    ModalHome,
}

impl FocusTarget {
    /// Stable egui id for this target.
    #[must_use]
    pub fn id(self) -> egui::Id {
        egui::Id::unique(("poker-focus", self))
    }
}

/// Home screen Tab order: the start button comes first (tabindex=1 in the
/// original page), then the secondary buttons and the settings controls.
#[must_use]
pub fn home_order() -> &'static [FocusTarget] {
    &[
        FocusTarget::StartGame,
        FocusTarget::OnlineGame,
        FocusTarget::OpenSettings,
        FocusTarget::VariantStandard,
        FocusTarget::VariantShortDeck,
        FocusTarget::Players,
        FocusTarget::Stack,
        FocusTarget::Blinds,
        FocusTarget::PlayerName,
        FocusTarget::AiDelay,
    ]
}

/// Table Tab order: top-bar toggles first, then the legal action buttons.
#[must_use]
pub fn table_order(human_turn: bool) -> Vec<FocusTarget> {
    let mut order = vec![
        FocusTarget::HideCards,
        FocusTarget::Music,
        FocusTarget::Analysis,
        FocusTarget::TableSettings,
    ];
    if human_turn {
        order.extend([
            FocusTarget::Fold,
            FocusTarget::Check,
            FocusTarget::Call,
            FocusTarget::Raise,
            FocusTarget::AllIn,
        ]);
    }
    order
}

/// Settlement modal focus trap order (Tab cycles only inside the modal).
#[must_use]
pub fn modal_order(game_over: bool) -> Vec<FocusTarget> {
    if game_over {
        vec![FocusTarget::ModalHome]
    } else {
        vec![FocusTarget::ModalNextHand, FocusTarget::ModalHome]
    }
}

/// Next index in a focus chain. None means "nothing focused", which selects
/// the first (Tab) or last (Shift+Tab) target.
#[must_use]
pub fn step(current: Option<usize>, len: usize, forward: bool) -> Option<usize> {
    if len == 0 {
        return None;
    }
    match current {
        None => Some(if forward { 0 } else { len - 1 }),
        Some(index) => {
            let index = index.min(len - 1);
            Some(if forward {
                (index + 1) % len
            } else {
                (index + len - 1) % len
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_first_tab_lands_on_start_game() {
        assert_eq!(home_order().first(), Some(&FocusTarget::StartGame));
    }

    #[test]
    fn tab_step_wraps_from_nothing_and_at_ends() {
        assert_eq!(step(None, 5, true), Some(0));
        assert_eq!(step(None, 5, false), Some(4));
        assert_eq!(step(Some(4), 5, true), Some(0));
        assert_eq!(step(Some(0), 5, false), Some(4));
        assert_eq!(step(Some(2), 5, true), Some(3));
        assert_eq!(step(Some(2), 5, false), Some(1));
        assert_eq!(step(Some(9), 5, true), Some(0), "out of range clamps");
        assert_eq!(step(None, 0, true), None);
    }

    #[test]
    fn table_order_includes_actions_only_on_my_turn() {
        let idle = table_order(false);
        assert!(idle.contains(&FocusTarget::HideCards));
        assert!(!idle.contains(&FocusTarget::Fold));
        let active = table_order(true);
        assert_eq!(active.first(), Some(&FocusTarget::HideCards));
        assert_eq!(active.last(), Some(&FocusTarget::AllIn));
        assert!(active.contains(&FocusTarget::Raise));
    }

    #[test]
    fn modal_order_traps_on_the_two_buttons() {
        assert_eq!(
            modal_order(false),
            vec![FocusTarget::ModalNextHand, FocusTarget::ModalHome]
        );
        assert_eq!(modal_order(true), vec![FocusTarget::ModalHome]);
        assert_eq!(step(Some(1), 2, true), Some(0));
        assert_eq!(step(Some(0), 2, false), Some(1));
    }

    #[test]
    fn focus_ids_are_stable_and_distinct() {
        assert_eq!(FocusTarget::StartGame.id(), FocusTarget::StartGame.id());
        assert_ne!(FocusTarget::StartGame.id(), FocusTarget::OnlineGame.id());
    }
}
