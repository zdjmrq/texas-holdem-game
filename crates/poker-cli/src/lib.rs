//! Simulation, benchmark and golden-corpus tooling for the Rust rewrite.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod agent;
pub mod sim;

pub use agent::{AgentView, PolicyAgent, PolicyKind};
pub use sim::{SimConfig, SimOutcome, run};
