//! Core poker primitives: cards, decks, deterministic RNG plumbing and the
//! standard/short-deck hand evaluator.
//!
//! This crate is a pure computation library: no I/O, no async, no global
//! mutable state. Randomness is always injected through [`RandomSource`].
//!
//! The evaluator mirrors the semantics of the original JS project's
//! js/probability-core.js evaluator, which was verified exhaustively against
//! the three other JS evaluators in the v1.5.8 code base (all 2,598,960
//! standard five-card combinations and all 376,992 short-deck five-card
//! combinations agree on rank and score). Scores are integers of the form
//! rank * 10^10 + v0 * 10^8 + v1 * 10^6 + v2 * 10^4 + v3 * 10^2 + v4, which
//! fit exactly in both IEEE-754 doubles and u64.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod card;
pub mod deck;
pub mod error;
pub mod eval;
pub mod rng;
pub mod variant;

pub use card::{Card, Rank, Suit};
pub use deck::Deck;
pub use error::{CardParseError, DeckError};
pub use eval::{EvaluatedHand, HandCategory, MAX_EVALUATED_CARDS, evaluate};
pub use rng::{ChaChaAdapter, RandomSource, RuntimeRng, XorShift32Rng, hash_seed};
pub use variant::Variant;
