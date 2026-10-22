//! Deterministic self-play simulation harness.
//!
//! This is the seed of the AI acceptance harness: a fixed seed must reproduce
//! the same table exactly, chip counts must be conserved every hand, and any
//! illegal/stuck state must be reported instead of being papered over.

use std::collections::BTreeMap;
use std::time::Instant;

use poker_core::{ChaChaAdapter, Variant};
use poker_engine::{Game, GameConfig, Phase, Player};
use serde::Serialize;

use crate::agent::{AgentView, PolicyAgent, PolicyKind};

/// Everything needed to run one simulation.
#[derive(Clone, Debug)]
pub struct SimConfig {
    /// Game variant.
    pub variant: Variant,
    /// Number of seats.
    pub players: usize,
    /// Requested number of hands.
    pub hands: u64,
    /// Master seed for deck shuffles and agent decisions.
    pub seed: u64,
    /// Starting stack per seat.
    pub stack: u64,
    /// Small blind (standard variant).
    pub small_blind: u64,
    /// Big blind (standard variant).
    pub big_blind: u64,
    /// Ante (short-deck variant).
    pub ante: u64,
    /// Minimum bet (short-deck variant).
    pub min_bet: u64,
    /// Scripted policy for every seat.
    pub policy: PolicyKind,
    /// Refill busted seats between hands so long simulations keep running.
    pub refill: bool,
}

impl SimConfig {
    /// Big blind or short-deck minimum bet, used for BB/100 reporting.
    #[must_use]
    pub const fn big_blind_equivalent(&self) -> u64 {
        if self.variant.is_short_deck() {
            self.min_bet
        } else {
            self.big_blind
        }
    }
}

/// JSON summary of a simulation.
#[derive(Clone, Debug, Serialize)]
pub struct SimOutcome {
    /// Variant name ("standard" / "shortdeck").
    pub variant: String,
    /// Seat count.
    pub players: usize,
    /// Requested hand count.
    pub requested_hands: u64,
    /// Hands that reached a settlement.
    pub hands_played: u64,
    /// Settlement reason counts (showdown / fold).
    pub hands_by_reason: BTreeMap<String, u64>,
    /// Action type counts.
    pub action_counts: BTreeMap<String, u64>,
    /// Any stuck/illegal state encountered (empty is good).
    pub errors: Vec<String>,
    /// Hands where total chips changed (must stay zero).
    pub chip_conservation_violations: u64,
    /// Starting stack per seat.
    pub starting_stacks: Vec<u64>,
    /// Final stack per seat.
    pub final_stacks: Vec<u64>,
    /// Big-blind-per-100-hands result per seat.
    pub bb_per_100: Vec<f64>,
    /// Wall-clock duration in milliseconds.
    pub duration_ms: u64,
    /// Why the simulation stopped before the requested hand count.
    pub stopped_early: Option<String>,
}

fn build_players(config: &SimConfig) -> Vec<Player> {
    (0..config.players)
        .map(|seat| {
            let id = if seat == 0 {
                "human".to_string()
            } else {
                format!("seat-{seat}")
            };
            let name = format!("Seat{seat}");
            Player::new(id, seat, name, config.stack).with_human(seat == 0)
        })
        .collect()
}

/// Runs a self-play simulation and returns its metrics.
#[must_use]
pub fn run(config: &SimConfig) -> SimOutcome {
    let started = Instant::now();
    let agents: Vec<PolicyAgent> = (0..config.players)
        .map(|_| PolicyAgent::new(config.policy))
        .collect();
    let mut game = Game::new(
        GameConfig {
            variant: config.variant,
            starting_stack: config.stack,
            small_blind: config.small_blind,
            big_blind: config.big_blind,
            ante: config.ante,
            min_bet: config.min_bet,
            max_players: config.players,
        },
        build_players(config),
    );
    let mut deck_rng = ChaChaAdapter::from_u64(config.seed);
    let mut agent_rng = ChaChaAdapter::from_u64(config.seed ^ 0x9e37_79b9_7f4a_7c15);

    let mut hands_played = 0u64;
    let mut hands_by_reason: BTreeMap<String, u64> = BTreeMap::new();
    let mut action_counts: BTreeMap<String, u64> = BTreeMap::new();
    let mut errors: Vec<String> = Vec::new();
    let mut chip_conservation_violations = 0u64;
    let mut stopped_early: Option<String> = None;
    let starting_stacks: Vec<u64> = game.players().iter().map(|player| player.stack).collect();

    for _hand in 0..config.hands {
        if config.refill {
            for player in game.players_mut() {
                if player.stack == 0 {
                    player.stack = config.stack;
                }
            }
        }
        let chips_before: u64 = game.players().iter().map(|player| player.stack).sum();
        if !game.start_hand(&mut deck_rng) {
            stopped_early = Some("fewer than two eligible seats".to_string());
            break;
        }
        let hand_index = hands_played;
        let mut guard = 0u64;
        let mut failed = false;
        while game.phase() != Phase::Idle {
            guard += 1;
            if guard > 2000 {
                errors.push(format!(
                    "hand {hand_index}: action guard tripped in phase {:?}",
                    game.phase()
                ));
                failed = true;
                break;
            }
            let seat = game.current_player_idx();
            if seat < 0 {
                errors.push(format!(
                    "hand {hand_index}: no current seat in phase {:?}",
                    game.phase()
                ));
                failed = true;
                break;
            }
            let seat = seat as usize;
            let legal = game.legal_actions(seat);
            if legal.actions.is_empty() {
                errors.push(format!(
                    "hand {hand_index}: empty legal action set for seat {seat} in phase {:?}",
                    game.phase()
                ));
                failed = true;
                break;
            }
            let view = AgentView {
                seat,
                legal: &legal,
                pot: game.pot(),
                phase: game.phase(),
                players: game.players(),
                community: game.community_cards(),
            };
            let action = agents[seat].decide(&view, &mut agent_rng);
            *action_counts
                .entry(action.action_type().as_str().to_string())
                .or_insert(0) += 1;
            if let Err(error) = game.act(seat, action) {
                errors.push(format!(
                    "hand {hand_index}: seat {seat} action rejected: {error}"
                ));
                failed = true;
                break;
            }
        }
        if failed {
            break;
        }
        hands_played += 1;
        if let Some(result) = game.last_hand_result() {
            *hands_by_reason
                .entry(result.reason.as_str().to_string())
                .or_insert(0) += 1;
        }
        let chips_after: u64 = game.players().iter().map(|player| player.stack).sum();
        if chips_after != chips_before {
            chip_conservation_violations += 1;
            errors.push(format!(
                "hand {hand_index}: chips changed from {chips_before} to {chips_after}"
            ));
        }
    }

    let big_blind = config.big_blind_equivalent().max(1);
    let final_stacks: Vec<u64> = game.players().iter().map(|player| player.stack).collect();
    let bb_per_100 = if hands_played == 0 {
        vec![0.0; config.players]
    } else {
        final_stacks
            .iter()
            .zip(starting_stacks.iter())
            .map(|(finish, start)| {
                #[allow(clippy::cast_precision_loss)]
                let delta = *finish as f64 - *start as f64;
                #[allow(clippy::cast_precision_loss)]
                let hands = hands_played as f64;
                #[allow(clippy::cast_precision_loss)]
                let blind = big_blind as f64;
                delta / blind / hands * 100.0
            })
            .collect()
    };
    #[allow(clippy::cast_possible_truncation)]
    let duration_ms = started.elapsed().as_millis() as u64;

    SimOutcome {
        variant: if config.variant.is_short_deck() {
            "shortdeck"
        } else {
            "standard"
        }
        .to_string(),
        players: config.players,
        requested_hands: config.hands,
        hands_played,
        hands_by_reason,
        action_counts,
        errors,
        chip_conservation_violations,
        starting_stacks,
        final_stacks,
        bb_per_100,
        duration_ms,
        stopped_early,
    }
}
