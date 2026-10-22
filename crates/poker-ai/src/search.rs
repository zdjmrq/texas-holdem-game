//! Production search budget. Golden replays keep their reference budgets.
use crate::input::{DecisionContext, RawDecisionContext};
/// Increase actual Monte Carlo work and CFR convergence, independently of animation pacing.
pub fn deepen(raw: &mut RawDecisionContext) {
    raw.mc_samples = Some(20_000.0);
    raw.river_iterations = Some(8_000);
    raw.time_budget_ms = Some(30_000.0);
}
/// Same budget for normalised local contexts.
pub fn deepen_context(context: &mut DecisionContext) {
    context.mc_samples = 20_000.0;
    context.river_iterations = 8_000;
    context.time_budget_ms = 30_000.0;
}
