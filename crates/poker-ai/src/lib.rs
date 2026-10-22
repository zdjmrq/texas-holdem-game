//! AI players for the Rust rewrite: personalities, opponent models, range
//! equity, the pre/post-flop decision pipeline, public-range river CFR and the
//! frozen 6+ short-deck strategy.
//!
//! Ported from the original JS project (v1.5.8):
//!
//! * js/ai-core.js - standard unified core (StandardBrain)
//! * js/ai-shortdeck-stable.js - frozen short-deck strategy (ShortDeckBrain)
//! * js/ai.js - the AIPlayer adapter
//! * js/river-solver.js + js/compiled-cfr.js - public-range river CFR (river)
//!
//! The behavioural contract, RNG consumption order, magic constants and the
//! golden corpus format live in docs/specs/ai-strategy.md. The crate is a pure
//! computation library: no I/O, no async, no wall-clock decisions.
#![forbid(unsafe_code)]

pub mod adapter;
pub mod belief;
pub mod equity;
pub mod input;
pub mod math;
pub mod model;
pub mod plan;
pub mod postflop;
pub mod preflop;
pub mod profile;
pub mod rng;
pub mod self_image;
pub mod shortdeck;
pub mod trace;

/// Public-range heads-up river CFR subgame (implementation owned by task-10).
pub mod river;

pub use adapter::{AIPlayer, StandardBrain};
pub use belief::OpponentBelief;
pub use input::{
    DecisionContext, LegalAction, ObservedAction, ObservedActionMeta, PublicPlayer, RiverDecision,
    RiverPolicyNode, RiverRequest, RiverSolution, RiverTrace, ShowdownResult, Street,
};
pub use model::BehavioralModel;
pub use plan::HandPlan;
pub use postflop::{PostflopCandidate, ResponseModel};
pub use profile::{AiProfile, ProfileOverrides, ProfileSource, Style};
pub use rng::AiRng;
pub use self_image::SelfImage;
pub use trace::{DecisionTrace, TraceCandidate, TraceEquity};

/// Errors returned by the AI crate.
#[derive(Clone, Debug, PartialEq)]
pub enum AiError {
    /// The decision context is malformed (for example a missing hole card).
    InvalidContext(&'static str),
    /// The river solver was requested but is not available yet.
    RiverUnavailable,
    /// The river solver rejected its inputs.
    RiverInvalid(&'static str),
    /// The short-deck brain is not available yet.
    ShortDeckUnavailable,
}

impl core::fmt::Display for AiError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            AiError::InvalidContext(message) => write!(f, "invalid AI context: {message}"),
            AiError::RiverUnavailable => write!(f, "river solver unavailable"),
            AiError::RiverInvalid(message) => write!(f, "invalid river solver input: {message}"),
            AiError::ShortDeckUnavailable => write!(f, "short-deck brain unavailable"),
        }
    }
}

impl std::error::Error for AiError {}

/// Common interface implemented by the standard, short-deck and (future) CFR
/// brains. The adapter owns observation routing; brains stay pure decision
/// engines over public information.
pub trait Brain {
    /// Decides an action from a normalised public decision context.
    fn decide(&mut self, context: &DecisionContext) -> DecisionWithTrace;

    /// Records one public action taken by any seat (including self).
    fn observe_action(&mut self, event: &ObservedAction);

    /// Records a showdown result revealed at the end of a hand.
    fn observe_showdown(&mut self, seat: usize, result: &ShowdownResult);

    /// Starts a new hand; per-hand beliefs are reset, cross-hand models persist.
    fn begin_hand(&mut self, hand_id: u64);
}

/// A decision plus its diagnostic trace (equivalent to the JS
/// {action, amount, trace} return value).
#[derive(Clone, Debug, PartialEq)]
pub struct DecisionWithTrace {
    /// Final legal action.
    pub action: poker_engine::ActionType,
    /// Final amount in chips (0 for fold/check; call amount; raise total).
    pub amount: u64,
    /// Trace produced by the last decision (always available for diagnostics).
    pub trace: DecisionTrace,
}
