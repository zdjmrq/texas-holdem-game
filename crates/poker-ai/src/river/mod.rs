//! Public-range heads-up river CFR subgame.
//!
//! Port of js/river-solver.js + js/compiled-cfr.js (v1.5.8) and the solver
//! adaptation in js/ai-core.js solveRiver (docs/specs/ai-strategy.md section 8).
//!
//! The solver is a pure computation: it receives the five public board cards,
//! the standard deck, both players' public-range weights and the river pot/stack
//! geometry, and returns the average-strategy policy plus the root action values
//! from that average policy. Chance is fully expanded by the six-bucket public
//! range abstraction, so the compiled CFR tree contains no chance nodes.
//!
//! Scope limits (kept from the original implementation):
//!
//! * heads-up river only, both players not all-in, equal pre-street commitments,
//!   no third live player and no dead money layer;
//! * standard deck only (short deck has no CFR);
//! * six private strength buckets, blockers inside one bucket are averaged;
//! * public bet menu of about 1/3, 2/3 and 1 pot plus the effective all-in, with
//!   at most one future raise;
//! * each decision re-solves from scratch with the fixed 800 iterations;
//! * the solve itself is deterministic and consumes no randomness - only the
//!   final average-strategy sampling uses one RNG draw.
//!
//! The adapter (StandardBrain) owns the gating rules and the public-range
//! weights; this module owns the range abstraction, the tree, the CFR tables and
//! the post-solve sampling helper.

pub mod cfr;
pub mod game;
pub mod ranges;

use std::collections::BTreeMap;

use poker_engine::ActionType;

use crate::AiError;
use crate::input::{
    DecisionContext, RiverDecision, RiverPolicyNode, RiverRequest, RiverSolution, RiverTrace,
};

/// Scope description copied from the original JS solver.
pub const SCOPE: &str = "heads-up river; six strength buckets; exact public-range card removal; at most one future raise; blockers within a bucket are averaged";

/// Fixed production iteration count (js/ai-core.js solveRiver passes 800).
pub const DEFAULT_ITERATIONS: u32 = 800;

/// True when the compiled solver is available.
#[must_use]
pub const fn is_available() -> bool {
    true
}

/// Solves the bounded heads-up river subgame for the given public ranges.
///
/// Chance is expanded into at most 36 joint strength-bucket deals with exact
/// card removal; the finite betting tree is compiled once and iterated 800 times
/// (or the requested count). The returned policy is the average strategy and the
/// root values are recomputed from that average policy, exactly like solve() in
/// the original JS solver.
///
/// # Errors
///
/// Returns AiError::RiverInvalid when the board, weights, pot/stack geometry or
/// iteration count is invalid.
#[allow(clippy::cast_possible_truncation)]
pub fn solve(request: &RiverRequest) -> Result<RiverSolution, AiError> {
    let abstraction = ranges::ranges(&request.board, &request.deck, &request.weights)?;
    let river_game = game::RiverGame::new(
        request.pot,
        request.paid,
        request.stacks,
        request.current_bet,
        request.min_raise_to,
        request.min_bet,
        request.unit,
        request.checked_before,
        request.can_raise,
        abstraction.deals,
    )?;
    let iterations = if request.iterations == 0 {
        DEFAULT_ITERATIONS
    } else {
        request.iterations
    };
    let mut solver = cfr::CompiledCfr::compile(&river_game)?;
    solver.run(iterations)?;
    let policy = solver.policy();

    // Evaluate the AVERAGE policy, not the final iteration's strategy.
    let mut values = solver.values.clone();
    for node in (0..solver.node_count()).rev() {
        let info_index = solver.info_index[node];
        if info_index < 0 {
            continue;
        }
        let info = &solver.infos[usize::try_from(info_index).expect("non-negative info index")];
        let node_policy = policy
            .get(&info.key)
            .ok_or(AiError::RiverInvalid("missing policy node"))?;
        let edge_offset =
            usize::try_from(solver.child_offset[node]).expect("non-negative child offset");
        let count = usize::try_from(solver.child_count[node]).expect("non-negative child count");
        let mut value = 0.0f64;
        for action in 0..count {
            let child = usize::try_from(solver.children[edge_offset + action])
                .expect("non-negative child index");
            value += node_policy.probabilities[action] * values[child];
        }
        values[node] = value;
    }

    // Root action values, weighted by the chance probability of each deal.
    let paid = request.paid;
    let base = request.pot - paid[0] - paid[1];
    let mut root_mass: BTreeMap<String, f64> = BTreeMap::new();
    let mut root_values: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    for &(node, probability) in &solver.roots {
        let info_index = solver.info_index[node];
        let info = &solver.infos[usize::try_from(info_index).expect("non-negative info index")];
        let key = info.key.clone();
        *root_mass.entry(key.clone()).or_insert(0.0) += probability;
        let values_for_key = root_values
            .entry(key)
            .or_insert_with(|| vec![0.0f64; info.actions.len()]);
        let edge_offset =
            usize::try_from(solver.child_offset[node]).expect("non-negative child offset");
        for (action, _) in info.actions.iter().enumerate() {
            let child = usize::try_from(solver.children[edge_offset + action])
                .expect("non-negative child index");
            values_for_key[action] += probability * values[child];
        }
    }
    for (key, action_values) in &mut root_values {
        let mass = root_mass[key];
        for value in action_values.iter_mut() {
            *value = (*value / mass + base / 2.0) + paid[0];
        }
    }

    Ok(RiverSolution {
        policy,
        root_values,
        bucket_by_score: abstraction.bucket_by_score,
        iterations: solver.iterations,
        information_sets: u32::try_from(solver.infos.len())
            .map_err(|_| AiError::RiverInvalid("game tree is too large"))?,
        tree_nodes: u32::try_from(solver.node_count())
            .map_err(|_| AiError::RiverInvalid("game tree is too large"))?,
        scope: SCOPE,
    })
}

/// Infers the checked-before flag from positions when no observation is stored.
///
/// Port of the positional fallback in js/ai-core.js:1120-1122: at a zero current
/// bet the other live seat must already have checked when the hero is in
/// position. At a non-zero current bet the flag is always false.
#[must_use]
pub fn checked_before_from_position(context: &DecisionContext) -> bool {
    if context.current_bet != 0.0 {
        return false;
    }
    let Some(villain_seat) = context.active_opponent_seats.first() else {
        return false;
    };
    let hero = context
        .public_players
        .iter()
        .find(|player| player.seat == context.hero_seat);
    let villain = context
        .public_players
        .iter()
        .find(|player| player.seat == *villain_seat);
    let (Some(hero), Some(villain)) = (hero, villain) else {
        return false;
    };
    let (Some(hero_position), Some(villain_position)) =
        (hero.position_from_button, villain.position_from_button)
    else {
        return false;
    };
    let order = |position: i32| {
        if position == 0 {
            i32::try_from(context.player_count).unwrap_or(i32::MAX)
        } else {
            position
        }
    };
    order(hero_position) > order(villain_position)
}

/// Samples the hero's average-strategy policy after a solve.
///
/// Port of js/ai-core.js:1125-1135 (the part of solveRiver that runs after the
/// solver returns): look up the hero bucket, draw one action from the mixed
/// strategy with the adapter's single RNG roll, translate the solver action to an
/// engine action, apply the odd-stack round-up rule for the effective cap, and
/// build the river solver trace.
///
/// The caller owns the gating rules and builds the request/weights; the roll must
/// be the decision RNG's next value sampled after the solve (the solver itself
/// consumes no randomness). This helper is pure and never legalizes the action:
/// the adapter legalizes afterwards, exactly like the original JS pipeline.
///
/// # Errors
///
/// Returns AiError::RiverInvalid when the hero's bucket or information set is
/// missing from the solution, or when the public seat snapshot is incomplete.
#[allow(clippy::cast_precision_loss, clippy::cast_possible_truncation)]
pub fn select_decision(
    solution: &RiverSolution,
    context: &DecisionContext,
    hero_score: u64,
    roll: f64,
) -> Result<RiverDecision, AiError> {
    let bucket = *solution
        .bucket_by_score
        .get(&hero_score)
        .ok_or(AiError::RiverInvalid(
            "river solver misses hero information set",
        ))?;
    let key = format!("0:{bucket}:");
    let node = solution.policy.get(&key).ok_or(AiError::RiverInvalid(
        "river solver misses hero information set",
    ))?;
    let action_values = solution.root_values.get(&key).ok_or(AiError::RiverInvalid(
        "river solver misses hero information set",
    ))?;

    // Sample the average strategy exactly like the original loop: subtract each
    // probability and take the first action whose cumulative mass reaches roll.
    let mut remaining = roll;
    let mut selected = node
        .actions
        .last()
        .ok_or(AiError::RiverInvalid("empty river action menu"))?
        .clone();
    for (index, probability) in node.probabilities.iter().enumerate() {
        remaining -= probability;
        if remaining <= 0.0 {
            selected = node.actions[index].clone();
            break;
        }
    }
    let (kind, target) = match selected.split_once(':') {
        Some((kind, target)) => (kind, Some(target)),
        None => (selected.as_str(), None),
    };
    let action = match kind {
        "bet" | "raise" => ActionType::Raise,
        "call" => ActionType::Call,
        "check" => ActionType::Check,
        "fold" => ActionType::Fold,
        _ => return Err(AiError::RiverInvalid("unknown river action")),
    };

    let hero = context
        .public_players
        .iter()
        .find(|player| player.seat == context.hero_seat)
        .ok_or(AiError::RiverInvalid(
            "river solver misses hero information set",
        ))?;
    let villain_seat = *context
        .active_opponent_seats
        .first()
        .ok_or(AiError::RiverInvalid("river solver needs one opponent"))?;
    let villain = context
        .public_players
        .iter()
        .find(|player| player.seat == villain_seat)
        .ok_or(AiError::RiverInvalid(
            "river solver misses hero information set",
        ))?;
    let hero_cap = hero.stack + hero.round_bet;
    let effective_cap = hero_cap.min(villain.stack + villain.round_bet);
    let target_amount = target.and_then(|value| value.parse::<f64>().ok());
    let amount = if target_amount == Some(effective_cap) && effective_cap < hero_cap {
        // Cover an opponent's odd remaining stack by rounding UP; rounding down
        // would leave chips behind and create a different terminal branch.
        (effective_cap / context.wager_unit).ceil() * context.wager_unit
    } else if action == ActionType::Call {
        context.to_call
    } else {
        target_amount.map_or(0.0, |value| if value.is_nan() { 0.0 } else { value })
    };
    let selected_index = node
        .actions
        .iter()
        .position(|item| *item == selected)
        .ok_or(AiError::RiverInvalid(
            "river solver misses hero information set",
        ))?;
    let ev = *action_values
        .get(selected_index)
        .ok_or(AiError::RiverInvalid(
            "river solver misses hero information set",
        ))?;
    let trace = RiverTrace {
        iterations: solution.iterations,
        information_sets: solution.information_sets,
        tree_nodes: solution.tree_nodes,
        bucket,
        checked_before: checked_before_from_position(context),
        policy: RiverPolicyNode {
            player: node.player,
            actions: node.actions.clone(),
            probabilities: node.probabilities.clone(),
        },
        action_values: action_values.clone(),
        scope: solution.scope.to_owned(),
    };
    Ok(RiverDecision {
        action,
        amount,
        ev,
        trace,
    })
}
