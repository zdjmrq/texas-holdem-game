//! Variant-agnostic Texas hold'em betting engine.
//!
//! The engine is a pure state machine: it owns the dealing/betting/settlement
//! state and never performs I/O, spawning or timing. Randomness (deck shuffle
//! and the random first dealer) is injected through [`poker_core::RandomSource`].
//!
//! Semantics are ported from the v1.5.8 original JS project:
//!
//! * `server/server-game.js` `ServerPokerGame` — the strict, headless,
//!   injectable-RNG implementation of the shared `js/engine.js` betting
//!   kernel; it is the canonical oracle for both variants.
//! * `js/game-rules-core.js` — raise grid, side pots, uncalled refunds,
//!   action metadata and decision identities ([\`rules\`]).
//!
//! Both standard (blinds, 52 cards) and 6+ short deck (antes plus a doubled
//! dealer ante, 36 cards, min bet = 2x ante) flows are supported through
//! [`GameConfig::variant`].
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod action;
pub mod config;
pub mod game;
pub mod phase;
pub mod player;
pub mod result;
pub mod rules;

pub use action::{ActError, Action, ActionType, LegalActions};
pub use config::GameConfig;
pub use game::Game;
pub use phase::Phase;
pub use player::{ActionLabel, LastAction, Player};
pub use result::{HandEndReason, HandPlayerResult, HandResult, PlayerIdentity};
pub use rules::{
    SidePot, UncalledRefund, calculate_side_pots, find_uncalled_refund, raise_bounds, snap_raise,
};
