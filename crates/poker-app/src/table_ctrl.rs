//! Pure table interaction logic: shortcut mapping, primary action selection,
//! raise-panel math and analysis masking.
//!
//! Behaviour follows docs/specs/ui.md §3.8 and §4 (keyboard priority table):
//! 1/left/down decrease the raise by one grid unit, 2/right/up increase it,
//! Space/Enter confirm and Esc cancels. Nothing here touches egui state, so it
//! is unit-testable without opening a window.

use eframe::egui;
use poker_engine::{Action, ActionType, LegalActions, Phase};

/// Commands produced by the global keyboard handler or the action bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TableCommand {
    Primary,
    Fold,
    Check,
    Call,
    AllIn,
    OpenRaise,
    CloseRaise,
    ConfirmRaise,
    RaiseStep(i64),
    NextHand,
    Back,
    ToggleHideCards,
    ToggleMusic,
    ToggleAnalysis,
    DismissOverlay,
}

/// Map a key press to a table command. Returns None when the key is not a
/// shortcut, or when a text input currently owns the keyboard.
pub fn shortcut_for(key: egui::Key, text_input_focused: bool) -> Option<TableCommand> {
    if text_input_focused {
        return None;
    }
    match key {
        egui::Key::Space | egui::Key::Enter => Some(TableCommand::Primary),
        egui::Key::F => Some(TableCommand::Fold),
        egui::Key::C => Some(TableCommand::Call),
        egui::Key::R => Some(TableCommand::OpenRaise),
        egui::Key::A => Some(TableCommand::AllIn),
        egui::Key::N => Some(TableCommand::NextHand),
        egui::Key::Escape => Some(TableCommand::DismissOverlay),
        egui::Key::Num1 | egui::Key::ArrowLeft | egui::Key::ArrowDown => {
            Some(TableCommand::RaiseStep(-1))
        }
        egui::Key::Num2 | egui::Key::ArrowRight | egui::Key::ArrowUp => {
            Some(TableCommand::RaiseStep(1))
        }
        _ => None,
    }
}

/// Take one fresh table key and consume table-key events before egui buttons
/// see them. Held-key repeats cannot silently act on a later turn or street.
pub fn take_shortcut(ctx: &egui::Context, raise_open: bool) -> Option<TableCommand> {
    if ctx.text_edit_focused() {
        return None;
    }
    let command_for = |key| {
        shortcut_for(key, false)
            .filter(|command| raise_open || !matches!(command, TableCommand::RaiseStep(_)))
    };
    ctx.input_mut(|input| {
        let command = input.events.iter().find_map(|event| {
            if let egui::Event::Key { key, pressed: true, repeat: false, modifiers, .. } = event
                && !modifiers.ctrl && !modifiers.alt && !modifiers.command
            {
                command_for(*key)
            } else { None }
        });
        input.events.retain(|event| {
            !matches!(event, egui::Event::Key { key, modifiers, .. }
                if !modifiers.ctrl && !modifiers.alt && !modifiers.command && command_for(*key).is_some())
        });
        command
    })
}

/// The Space/Enter action: check when possible, otherwise call.
pub fn primary_action(legal: &LegalActions) -> Option<Action> {
    if legal.contains(ActionType::Check) {
        Some(Action::Check)
    } else if legal.contains(ActionType::Call) {
        Some(Action::Call)
    } else {
        None
    }
}

/// Initial raise amount: three big blinds pre-flop, otherwise min raise + two
/// big blinds, snapped onto the legal grid (ui.md §3.8 local rules).
#[must_use]
pub fn default_raise_amount(legal: &LegalActions, phase: Phase, big_blind: u64) -> u64 {
    let raw = if phase == Phase::Preflop {
        big_blind.saturating_mul(3)
    } else {
        legal
            .min_raise_to
            .saturating_add(big_blind.saturating_mul(2))
    };
    snap_to_grid(
        raw.max(legal.min_raise_to),
        legal.min_raise_to,
        legal.max_raise_to,
        legal.wager_unit,
    )
}

/// State of the raise panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RaisePanel {
    pub open: bool,
    pub amount: u64,
    pub min: u64,
    pub max: u64,
    pub unit: u64,
}

impl Default for RaisePanel {
    fn default() -> Self {
        Self {
            open: false,
            amount: 0,
            min: 0,
            max: 0,
            unit: 1,
        }
    }
}

impl RaisePanel {
    /// Open the panel with the legal range of the current decision point.
    #[must_use]
    pub fn open_for(legal: &LegalActions) -> Self {
        let min = legal.min_raise_to.max(1);
        let max = legal.max_raise_to.max(min);
        let unit = legal.wager_unit.max(1);
        Self {
            open: true,
            amount: snap_to_grid(min, min, max, unit),
            min,
            max,
            unit,
        }
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    /// Refresh the bounds from the current legal set and re-clamp the amount.
    pub fn set_bounds(&mut self, legal: &LegalActions) {
        self.min = legal.min_raise_to.max(1);
        self.max = legal.max_raise_to.max(self.min);
        self.unit = legal.wager_unit.max(1);
        self.amount = snap_to_grid(self.amount, self.min, self.max, self.unit);
    }

    /// Move the amount by whole grid steps (1/2 and arrow keys, slider).
    pub fn step(&mut self, units: i64) {
        let delta = units.saturating_mul(i64::try_from(self.unit).unwrap_or(i64::MAX));
        let next = i64::try_from(self.amount)
            .unwrap_or(i64::MAX)
            .saturating_add(delta);
        let next = u64::try_from(next.max(0)).unwrap_or(0);
        self.amount = snap_to_grid(next, self.min, self.max, self.unit);
    }

    /// Set an arbitrary amount, snapping it onto the legal grid.
    pub fn set_amount(&mut self, amount: u64) {
        self.amount = snap_to_grid(amount, self.min, self.max, self.unit);
    }

    /// The raise action represented by the current amount.
    #[must_use]
    pub fn action(&self) -> Action {
        Action::Raise { to: self.amount }
    }

    /// Human readable relation to the pot, for example "0.6x 底池".
    #[must_use]
    pub fn pot_fraction_label(&self, pot: u64) -> String {
        if pot == 0 {
            return "-".to_owned();
        }
        format!("{:.2}x 底池", self.amount as f64 / pot as f64)
    }

    /// Whether the amount is exactly the smallest legal raise.
    #[must_use]
    pub fn at_min(&self) -> bool {
        self.amount <= self.min
    }

    /// Whether the amount is exactly the largest legal raise.
    #[must_use]
    pub fn at_max(&self) -> bool {
        self.amount >= self.max
    }
}

/// Snap an amount onto the raise grid and clamp it into the legal range.
#[must_use]
pub fn snap_to_grid(amount: u64, min: u64, max: u64, unit: u64) -> u64 {
    poker_engine::snap_raise(amount as f64, min.max(1), max, unit.max(1))
        .unwrap_or(min.max(1))
        .clamp(min.max(1), max.max(min.max(1)))
}

/// What the analysis panel is allowed to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnalysisMask {
    Visible,
    /// The human hid their hole cards: never reveal analysis that depends on them.
    HiddenCards,
    /// The analysis panel was switched off in settings.
    Disabled,
}

/// Decide whether the analysis panel must be masked.
#[must_use]
pub fn analysis_mask(hide_hole_cards: bool, show_analysis: bool) -> AnalysisMask {
    if hide_hole_cards {
        AnalysisMask::HiddenCards
    } else if !show_analysis {
        AnalysisMask::Disabled
    } else {
        AnalysisMask::Visible
    }
}

pub fn visible_hand_name(hidden: bool, name: Option<&'static str>) -> Option<&'static str> {
    if hidden { None } else { name }
}

/// Format a chip amount together with its big-blind equivalent.
#[must_use]
pub fn format_bb(chips: u64, big_blind: u64) -> String {
    if big_blind == 0 {
        return chips.to_string();
    }
    format!("{} ({:.1} BB)", chips, chips as f64 / big_blind as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_keys_are_consumed_once_and_held_keys_cannot_act_again() {
        let ctx = egui::Context::default();
        let key = |key, repeat| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat,
            modifiers: egui::Modifiers::NONE,
        };
        let mut output = ctx.run_ui(
            egui::RawInput {
                events: vec![key(egui::Key::Space, false)],
                ..Default::default()
            },
            |ui| {
                let ctx = ui.ctx();
                assert_eq!(take_shortcut(ctx, false), Some(TableCommand::Primary));
                assert_eq!(take_shortcut(ctx, false), None);
                assert!(
                    !ctx.input(|i| i.key_pressed(egui::Key::Space)),
                    "focused buttons must not activate too"
                );
            },
        );
        output.textures_delta.clear();
        let mut output = ctx.run_ui(
            egui::RawInput {
                events: vec![key(egui::Key::Space, true)],
                ..Default::default()
            },
            |ui| {
                let ctx = ui.ctx();
                assert_eq!(take_shortcut(ctx, false), None);
                assert!(!ctx.input(|i| i.key_pressed(egui::Key::Space)));
            },
        );
        output.textures_delta.clear();
        let mut output = ctx.run_ui(
            egui::RawInput {
                events: vec![key(egui::Key::F, false), key(egui::Key::Enter, false)],
                ..Default::default()
            },
            |ui| {
                let ctx = ui.ctx();
                assert_eq!(take_shortcut(ctx, false), Some(TableCommand::Fold));
                assert_eq!(
                    take_shortcut(ctx, false),
                    None,
                    "a frame may submit only one keyboard command"
                );
            },
        );
        output.textures_delta.clear();
    }

    fn legal(min: u64, max: u64, unit: u64) -> LegalActions {
        LegalActions {
            actions: vec![ActionType::Call, ActionType::Raise],
            to_call: 100,
            can_raise: true,
            min_raise_to: min,
            max_raise_to: max,
            wager_unit: unit,
        }
    }

    #[test]
    fn primary_action_prefers_check_then_call() {
        let legal_check = LegalActions {
            actions: vec![ActionType::Check, ActionType::Raise],
            to_call: 0,
            can_raise: true,
            min_raise_to: 100,
            max_raise_to: 1000,
            wager_unit: 50,
        };
        assert_eq!(primary_action(&legal_check), Some(Action::Check));
        let legal_call = legal(100, 1000, 50);
        assert_eq!(primary_action(&legal_call), Some(Action::Call));
        let empty = LegalActions {
            actions: Vec::new(),
            to_call: 0,
            can_raise: false,
            min_raise_to: 0,
            max_raise_to: 0,
            wager_unit: 1,
        };
        assert_eq!(primary_action(&empty), None);
    }

    #[test]
    fn raise_panel_steps_on_the_grid_and_clamps() {
        let mut panel = RaisePanel::open_for(&legal(300, 1200, 100));
        assert_eq!(panel.amount, 300);
        panel.step(2);
        assert_eq!(panel.amount, 500);
        panel.step(-10);
        assert_eq!(panel.amount, 300);
        panel.step(100);
        assert_eq!(panel.amount, 1200);
        panel.set_amount(980);
        assert_eq!(panel.amount, 1000);
        panel.set_amount(1111);
        assert_eq!(panel.amount, 1100);
        panel.set_amount(0);
        assert_eq!(panel.amount, 300);
        assert!(panel.at_min());
        panel.set_amount(u64::MAX);
        assert_eq!(panel.amount, 1200);
        assert!(panel.at_max());
    }

    #[test]
    fn default_raise_amount_matches_ui_spec() {
        // Pre-flop: 3 BB snapped to the grid.
        let preflop = legal(200, 10_000, 100);
        assert_eq!(default_raise_amount(&preflop, Phase::Preflop, 200), 600);
        // Post-flop: min raise + 2 BB, snapped.
        let postflop = legal(200, 10_000, 100);
        assert_eq!(default_raise_amount(&postflop, Phase::Flop, 200), 600);
        // Clamped to the maximum when the stack is short.
        let short = legal(200, 400, 100);
        assert_eq!(default_raise_amount(&short, Phase::River, 200), 400);
    }

    #[test]
    fn shortcut_mapping_respects_text_input_focus_and_ui_spec() {
        assert_eq!(shortcut_for(egui::Key::F, false), Some(TableCommand::Fold));
        assert_eq!(shortcut_for(egui::Key::F, true), None);
        assert_eq!(
            shortcut_for(egui::Key::Num1, false),
            Some(TableCommand::RaiseStep(-1))
        );
        assert_eq!(
            shortcut_for(egui::Key::ArrowRight, false),
            Some(TableCommand::RaiseStep(1))
        );
        assert_eq!(
            shortcut_for(egui::Key::ArrowDown, false),
            Some(TableCommand::RaiseStep(-1))
        );
        assert_eq!(shortcut_for(egui::Key::Z, false), None);
    }

    #[test]
    fn analysis_masking_prioritises_hidden_cards() {
        assert_eq!(analysis_mask(false, true), AnalysisMask::Visible);
        assert_eq!(analysis_mask(true, true), AnalysisMask::HiddenCards);
        assert_eq!(analysis_mask(false, false), AnalysisMask::Disabled);
        assert_eq!(analysis_mask(true, false), AnalysisMask::HiddenCards);
    }

    #[test]
    fn hidden_cards_also_mask_action_bar_hand_type() {
        assert_eq!(visible_hand_name(true, Some("皇家同花顺")), None);
        assert_eq!(
            visible_hand_name(false, Some("皇家同花顺")),
            Some("皇家同花顺")
        );
    }

    #[test]
    fn extreme_keyboard_steps_do_not_overflow() {
        let mut panel = RaisePanel::open_for(&legal(300, 1200, 100));
        panel.step(i64::MAX);
        assert_eq!(panel.amount, 1200);
        panel.step(i64::MIN);
        assert_eq!(panel.amount, 300);
    }

    #[test]
    fn bb_formatting_rounds_to_one_decimal() {
        assert_eq!(format_bb(5_000, 200), "5000 (25.0 BB)");
        assert_eq!(format_bb(250, 200), "250 (1.2 BB)");
        assert_eq!(format_bb(40, 0), "40");
    }
}
