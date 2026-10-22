//! Golden replay and self-tests for the public-range river CFR solver.
//!
//! Scope: task-10 (Phase 4, river CFR). The test replays the 13 synthetic river
//! CFR decisions of tests/golden/ai/decisions-synthetic.jsonl:
//!
//! * the solver request is rebuilt from the recorded public context exactly like
//!   js/ai-core.js solveRiver builds it (pot, paid, stacks, current bet, min
//!   raise, big blind, wager unit, checked-before and public-range weights);
//! * the public-range weights are rebuilt in Rust with crate::belief (hero range:
//!   fresh public self belief, opponent range: belief + behavioural model fed by
//!   the recorded observation events), reproducing the JS comboWeight path;
//! * the average-strategy policy, root action values, information-set count, tree
//!   node count and hero bucket are compared against the recorded JS output
//!   (policy tolerance 1e-9, action values relative 1e-6, structure exact);
//! * the decision is completed through river::select_decision with the same RNG
//!   roll the JS brain used (the last of expect.rngCalls draws from the seeded
//!   decision stream) and legalized with AIPlayer::legalize; the final
//!   action/amount must match the corpus exactly.
//!
//! Additional self-tests cover the range abstraction (1081 combinations, 36
//! deals, equal-score grouping, exact card removal, input validation), the finite
//! betting tree (menus, payoff signs, zero-sum), a Kuhn poker equilibrium
//! (game value -1/18), an independent recursive tabular CFR oracle compared with
//! the compiled CFR implementation, the select_decision contract, and a simple
//! timing budget for the production 800-iteration solve.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::time::Instant;

use poker_ai::adapter::AIPlayer;
use poker_ai::belief::OpponentBelief;
use poker_ai::input::{
    DecisionContext, ObservedAction, RawDecisionContext, RiverPolicyNode, RiverSolution,
};
use poker_ai::math::BoardComboFeatures;
use poker_ai::model::BehavioralModel;
use poker_ai::profile::{ProfileSource, Style, profile_for};
use poker_ai::river::cfr::{CompiledCfr, GameTree};
use poker_ai::river::game::{End, RiverGame, State};
use poker_ai::river::{self, SCOPE, ranges};
use poker_ai::rng::AiRng;
use poker_ai::{AiError, RiverRequest};
use poker_core::{Card, Deck, Variant, evaluate};
use serde::Deserialize;

// ---------------------------------------------------------------------------
// Golden corpus model
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CorpusAi {
    variant: String,
    style: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CorpusAiState {
    #[serde(deserialize_with = "poker_ai::input::card_vec::deserialize")]
    hole_cards: Vec<Card>,
}

#[derive(Deserialize)]
struct CorpusObserve {
    op: String,
    #[serde(default)]
    event: Option<ObservedAction>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CorpusExpect {
    action: String,
    amount: f64,
    rng_calls: u64,
    trace: CorpusTrace,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CorpusTrace {
    river_solver: ExpectedRiver,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExpectedRiver {
    iterations: u32,
    information_sets: u32,
    tree_nodes: u32,
    bucket: u8,
    checked_before: bool,
    policy: ExpectedPolicy,
    action_values: Vec<f64>,
    scope: String,
}

#[derive(Deserialize)]
struct ExpectedPolicy {
    player: u8,
    actions: Vec<String>,
    probabilities: Vec<f64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CorpusRecord {
    id: String,
    ai: CorpusAi,
    ai_state: CorpusAiState,
    observes: Vec<CorpusObserve>,
    context: RawDecisionContext,
    expect: CorpusExpect,
}

fn corpus_path() -> PathBuf {
    PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/golden/ai/decisions-synthetic.jsonl"
    ))
}

fn load_river_records() -> Vec<CorpusRecord> {
    let text = std::fs::read_to_string(corpus_path()).expect("read decisions-synthetic.jsonl");
    let mut records = Vec::new();
    for line in text.lines() {
        let value: serde_json::Value = serde_json::from_str(line).expect("corpus line parses");
        let is_river_cfr = value["tags"]
            .as_array()
            .is_some_and(|tags| tags.iter().any(|tag| tag.as_str() == Some("river-cfr")));
        if is_river_cfr {
            records.push(serde_json::from_value(value).expect("river-cfr record parses"));
        }
    }
    records
}

fn record<'a>(records: &'a [CorpusRecord], id: &str) -> &'a CorpusRecord {
    records
        .iter()
        .find(|record| record.id == id)
        .unwrap_or_else(|| panic!("corpus record {id} missing"))
}

// ---------------------------------------------------------------------------
// Request reconstruction (js/ai-core.js solveRiver, lines 1109-1136)
// ---------------------------------------------------------------------------

/// A solved corpus decision plus the inputs that produced it.
struct Reconstructed {
    context: DecisionContext,
    request: RiverRequest,
    hero_score: u64,
    style: Style,
}

/// Rebuilds the opponent (and public self) perception weights in Rust.
///
/// The hero's range is the fresh public self belief used by solveRiver when the
/// brain has never observed its own actions; the opponent range is the belief
/// updated by the recorded public observation events. Configuration values come
/// from the adapter profile (standard target table), matching ensureOpponent.
fn reconstruct_weights(
    context: &DecisionContext,
    style: Style,
    observes: &[CorpusObserve],
) -> Vec<(f64, f64)> {
    let profile = profile_for(style, Variant::Standard, ProfileSource::StandardStyle);
    let hero_seat = context.hero_seat;
    let villain_seat = context.active_opponent_seats[0];
    let mut self_belief = OpponentBelief::new(hero_seat);
    self_belief.configure(
        profile.range_evidence_weight,
        profile.range_percentile_weight,
        profile.preflop_range_sharpness,
        profile.observed_preflop_range_weight,
    );
    let mut self_model = BehavioralModel::new(hero_seat);
    let mut opponent_belief = OpponentBelief::new(villain_seat);
    opponent_belief.configure(
        profile.range_evidence_weight,
        profile.range_percentile_weight,
        profile.preflop_range_sharpness,
        profile.observed_preflop_range_weight,
    );
    let mut opponent_model = BehavioralModel::new(villain_seat);
    let mut self_history = false;
    let mut opponent_history = false;
    for observe in observes {
        assert_eq!(
            observe.op, "observe",
            "river-cfr expects action observations"
        );
        let Some(event) = observe.event.as_ref() else {
            continue;
        };
        if event.seat_id == hero_seat {
            self_history = true;
            self_model.observe(event);
            self_belief.observe(event, &self_model);
        } else {
            opponent_history = true;
            opponent_model.observe(event);
            opponent_belief.observe(event, &opponent_model);
        }
    }

    let deck = Deck::standard();
    let mut board_mask = 0u64;
    for card in &context.community_cards {
        board_mask |= card.bit();
    }
    let available: Vec<Card> = deck
        .cards()
        .iter()
        .copied()
        .filter(|card| board_mask & card.bit() == 0)
        .collect();
    assert_eq!(available.len(), 47, "standard river has 47 available cards");
    let mut cache: HashMap<String, BoardComboFeatures> = HashMap::new();
    let mut weights = Vec::with_capacity(available.len() * (available.len() - 1) / 2);
    for (index, &first) in available.iter().enumerate() {
        for &second in &available[(index + 1)..] {
            let self_weight = self_belief.combo_weight(
                first,
                second,
                Variant::Standard,
                if self_history {
                    Some(&self_model)
                } else {
                    None
                },
                &mut cache,
            );
            let opponent_weight = opponent_belief.combo_weight(
                first,
                second,
                Variant::Standard,
                if opponent_history {
                    Some(&opponent_model)
                } else {
                    None
                },
                &mut cache,
            );
            weights.push((self_weight, opponent_weight));
        }
    }
    weights
}

/// Last observed river action, used for the checked-before decision.
fn last_river_observation(observes: &[CorpusObserve]) -> Option<&ObservedAction> {
    observes
        .iter()
        .filter_map(|observe| observe.event.as_ref())
        .rfind(|event| event.street == poker_ai::input::Street::River)
}

fn reconstruct(record: &CorpusRecord) -> Reconstructed {
    let style = Style::from_name(&record.ai.style).expect("known style");
    assert_eq!(
        record.ai.variant, "standard",
        "river CFR only exists for standard"
    );
    let mut context = record.context.clone().normalize();
    // _decideShared injects the adapter's hole cards into the brain context.
    context.hole_cards = record.ai_state.hole_cards.clone();
    assert_eq!(context.hole_cards.len(), 2);
    assert_eq!(context.community_cards.len(), 5);
    let hero_seat = context.hero_seat;
    let villain_seat = context.active_opponent_seats[0];
    let hero = *context
        .public_players
        .iter()
        .find(|player| player.seat == hero_seat)
        .expect("hero public row");
    let villain = *context
        .public_players
        .iter()
        .find(|player| player.seat == villain_seat)
        .expect("villain public row");
    let checked_before = if context.current_bet == 0.0 {
        match last_river_observation(&record.observes) {
            Some(event) => {
                event.seat_id == villain_seat && event.action.eq_ignore_ascii_case("check")
            }
            None => river::checked_before_from_position(&context),
        }
    } else {
        false
    };
    let weights = reconstruct_weights(&context, style, &record.observes);
    let board: [Card; 5] = context
        .community_cards
        .clone()
        .try_into()
        .expect("five board cards");
    let mut seven = [Card::new(poker_core::Suit::Spades, poker_core::Rank::Two); 7];
    seven[0] = context.hole_cards[0];
    seven[1] = context.hole_cards[1];
    seven[2..].copy_from_slice(&board);
    let hero_score = evaluate(&seven, Variant::Standard).score();
    let request = RiverRequest {
        board,
        deck: Deck::standard().cards().to_vec(),
        pot: context.pot,
        paid: [hero.round_bet, villain.round_bet],
        stacks: [hero.stack, villain.stack],
        current_bet: context.current_bet,
        min_raise_to: context.min_raise_to,
        min_bet: context.big_blind,
        unit: context.wager_unit,
        checked_before,
        can_raise: context.can_raise,
        iterations: 800,
        weights,
    };
    Reconstructed {
        context,
        request,
        hero_score,
        style,
    }
}

fn seed_text(reconstructed: &Reconstructed) -> String {
    let context = &reconstructed.context;
    let table_seed = if context.seed.is_empty() {
        "table"
    } else {
        context.seed.as_str()
    };
    format!(
        "{table_seed}:{}:{}:{}:{}",
        context.hand_id,
        context.hero_seat,
        context.decision_id,
        reconstructed.style.as_str()
    )
}

/// Reproduces the exact RNG roll used by solveRiver sampling: the solve consumes
/// no randomness and its single draw is the last draw of the decision stream.
fn decision_roll(reconstructed: &Reconstructed, rng_calls: u64) -> f64 {
    assert!(rng_calls > 0, "river CFR sampling always consumes one draw");
    let mut rng = AiRng::from_seed(&seed_text(reconstructed));
    let mut roll = f64::NAN;
    for _ in 0..rng_calls {
        roll = rng.next_f64();
    }
    roll
}

// ---------------------------------------------------------------------------
// Numeric comparison helpers
// ---------------------------------------------------------------------------

fn max_abs_diff(actual: &[f64], expected: &[f64]) -> f64 {
    if actual.len() != expected.len() {
        return f64::INFINITY;
    }
    actual
        .iter()
        .zip(expected)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f64, f64::max)
}

fn max_relative_diff(actual: &[f64], expected: &[f64]) -> f64 {
    if actual.len() != expected.len() {
        return f64::INFINITY;
    }
    actual
        .iter()
        .zip(expected)
        .map(|(a, b)| {
            let diff = (a - b).abs();
            if diff == 0.0 {
                0.0
            } else {
                diff / a.abs().max(b.abs()).max(1e-12)
            }
        })
        .fold(0.0f64, f64::max)
}

// ---------------------------------------------------------------------------
// Corpus replay
// ---------------------------------------------------------------------------

#[test]
fn river_cfr_matches_golden_corpus() {
    let records = load_river_records();
    assert_eq!(records.len(), 13, "expected 13 river-cfr decisions");

    let mut failures: Vec<String> = Vec::new();
    let mut max_policy_diff = 0.0f64;
    let mut max_value_diff = 0.0f64;
    let mut actions_matched = 0usize;
    let mut structures_matched = 0usize;

    for record in &records {
        let expected = &record.expect.trace.river_solver;
        let reconstructed = reconstruct(record);
        let solution = river::solve(&reconstructed.request).unwrap_or_else(|error| {
            panic!(
                "{}: solver rejected the reconstructed request: {error}",
                record.id
            )
        });
        let mut problems: Vec<String> = Vec::new();

        // Compiled tree structure must be identical (hard contract).
        let structure_ok = solution.iterations == expected.iterations
            && solution.information_sets == expected.information_sets
            && solution.tree_nodes == expected.tree_nodes;
        if solution.iterations != expected.iterations {
            problems.push(format!(
                "iterations {} != {}",
                solution.iterations, expected.iterations
            ));
        }
        if solution.information_sets != expected.information_sets {
            problems.push(format!(
                "informationSets {} != {}",
                solution.information_sets, expected.information_sets
            ));
        }
        if solution.tree_nodes != expected.tree_nodes {
            problems.push(format!(
                "treeNodes {} != {}",
                solution.tree_nodes, expected.tree_nodes
            ));
        }
        if structure_ok {
            structures_matched += 1;
        }
        if solution.scope != expected.scope {
            problems.push("scope string differs".to_owned());
        }

        // Hero information set: bucket, menu and average strategy.
        let bucket = match solution.bucket_by_score.get(&reconstructed.hero_score) {
            Some(bucket) => *bucket,
            None => {
                problems.push("hero score has no bucket".to_owned());
                continue;
            }
        };
        if bucket != expected.bucket {
            problems.push(format!("hero bucket {bucket} != {}", expected.bucket));
        }
        let key = format!("0:{bucket}:");
        let Some(node) = solution.policy.get(&key) else {
            problems.push(format!("missing policy node {key}"));
            continue;
        };
        if node.player != expected.policy.player {
            problems.push(format!(
                "policy player {} != {}",
                node.player, expected.policy.player
            ));
        }
        if node.actions != expected.policy.actions {
            problems.push(format!(
                "policy actions {:?} != {:?}",
                node.actions, expected.policy.actions
            ));
        }
        let policy_diff = max_abs_diff(&node.probabilities, &expected.policy.probabilities);
        max_policy_diff = max_policy_diff.max(policy_diff);
        if policy_diff > 1e-9 {
            problems.push(format!("policy probability diff {policy_diff:e}"));
        }
        let probability_sum: f64 = node.probabilities.iter().sum();
        if (probability_sum - 1.0).abs() > 1e-9 {
            problems.push(format!("policy probabilities sum to {probability_sum}"));
        }
        let Some(action_values) = solution.root_values.get(&key) else {
            problems.push(format!("missing root values {key}"));
            continue;
        };
        let value_diff = max_relative_diff(action_values, &expected.action_values);
        max_value_diff = max_value_diff.max(value_diff);
        if value_diff > 1e-6 {
            problems.push(format!("root action value rel diff {value_diff:e}"));
        }

        // Full decision: sample the average policy with the recorded RNG roll and
        // legalize it, exactly like the JS pipeline does after solveRiver.
        let roll = decision_roll(&reconstructed, record.expect.rng_calls);
        let decision = river::select_decision(
            &solution,
            &reconstructed.context,
            reconstructed.hero_score,
            roll,
        )
        .unwrap_or_else(|error| panic!("{}: select_decision failed: {error}", record.id));
        let (action, amount) =
            AIPlayer::legalize(&reconstructed.context, decision.action, decision.amount);
        let action_matches = action.as_str() == record.expect.action.as_str()
            && (amount_as_f64(amount) - record.expect.amount).abs() < 1e-9;
        if !action_matches {
            problems.push(format!(
                "decision {}/{} != {}/{}",
                action.as_str(),
                amount,
                record.expect.action,
                record.expect.amount
            ));
        } else {
            actions_matched += 1;
        }
        // The decision trace must carry the solved information.
        let trace = &decision.trace;
        if trace.iterations != expected.iterations
            || trace.information_sets != expected.information_sets
            || trace.tree_nodes != expected.tree_nodes
            || trace.bucket != expected.bucket
            || trace.checked_before != expected.checked_before
        {
            problems.push("river trace metadata differs".to_owned());
        }
        if trace.policy.actions != expected.policy.actions {
            problems.push("river trace policy actions differ".to_owned());
        }
        let trace_policy_diff =
            max_abs_diff(&trace.policy.probabilities, &expected.policy.probabilities);
        if trace_policy_diff > 1e-9 {
            problems.push(format!("river trace policy diff {trace_policy_diff:e}"));
        }
        let trace_value_diff = max_relative_diff(&trace.action_values, &expected.action_values);
        if trace_value_diff > 1e-6 {
            problems.push(format!("river trace value diff {trace_value_diff:e}"));
        }
        if trace.scope != expected.scope {
            problems.push("river trace scope differs".to_owned());
        }

        eprintln!(
            "{}: policy_diff={policy_diff:e} value_rel_diff={value_diff:e} action={}{}",
            record.id,
            if action_matches { "ok" } else { "MISMATCH" },
            if problems.is_empty() { "" } else { " PROBLEMS" }
        );
        if !problems.is_empty() {
            failures.push(format!("{}: {}", record.id, problems.join("; ")));
        }
    }

    eprintln!(
        "river CFR corpus: {}/13 decisions matched, {structures_matched}/13 structures, policy_max={max_policy_diff:e}, value_max_rel={max_value_diff:e}",
        actions_matched
    );
    assert!(
        failures.is_empty(),
        "river CFR corpus failures:\n{}",
        failures.join("\n")
    );
    assert_eq!(structures_matched, 13);
    assert_eq!(actions_matched, 13);
}

fn amount_as_f64(amount: u64) -> f64 {
    #[allow(clippy::cast_precision_loss)]
    {
        amount as f64
    }
}

// ---------------------------------------------------------------------------
// Per-combination weight spot checks (locks the belief layer)
// ---------------------------------------------------------------------------

/// Expected opponent-range weights captured from the JS solver inputs for the
/// six "checked" synthetic records (the seven uniform records have no observed
/// history and therefore uniformly return 1.0).
///
/// Values are the JS comboWeight output over the 1081 available combinations in
/// deck order: (min, max, mean, spot weights at indices 0, 1, 37, 100, 250, 500,
/// 700, 900, 1000, 1050, 1070, 1080).
const CHECKED_WEIGHT_SUMMARY: [(&str, f64, f64, f64, [f64; 12]); 6] = [
    (
        "synthetic-cfr-river-dry-1-checked",
        0.930_951_517_141_935_9,
        1.073207348649492,
        1.0435179332649789,
        [
            1.0378233110439326,
            1.0378233110439326,
            1.0378233110439326,
            1.0378233110439326,
            0.984_554_376_181_505_5,
            1.0378233110439326,
            1.073207348649492,
            1.0378233110439326,
            1.073207348649492,
            1.073207348649492,
            1.073207348649492,
            1.073207348649492,
        ],
    ),
    (
        "synthetic-cfr-river-dry-2-checked",
        0.930_951_517_141_935_9,
        1.073207348649492,
        1.0199986185634917,
        [
            1.0378233110439326,
            1.0378233110439326,
            1.0378233110439326,
            1.0378233110439326,
            1.0378233110439326,
            1.0378233110439326,
            1.0378233110439326,
            1.073207348649492,
            1.073207348649492,
            1.073207348649492,
            1.0378233110439326,
            1.073207348649492,
        ],
    ),
    (
        "synthetic-cfr-river-dry-3-checked",
        0.930_951_517_141_935_9,
        1.0594786490859103,
        1.0195183866245954,
        [
            0.984_554_376_181_505_5,
            0.984_554_376_181_505_5,
            0.984_554_376_181_505_5,
            0.984_554_376_181_505_5,
            0.984_554_376_181_505_5,
            0.984_554_376_181_505_5,
            1.0594786490859103,
            1.0594786490859103,
            0.984_554_376_181_505_5,
            1.0594786490859103,
            1.0594786490859103,
            1.0594786490859103,
        ],
    ),
    (
        "synthetic-cfr-river-dry-4-checked",
        0.930_951_517_141_935_9,
        1.073207348649492,
        1.001_346_300_072_571,
        [
            1.0378233110439326,
            1.0378233110439326,
            1.0378233110439326,
            1.073207348649492,
            1.0378233110439326,
            1.0378233110439326,
            0.930_951_517_141_935_9,
            1.0378233110439326,
            1.0062759188940589,
            1.073207348649492,
            1.073207348649492,
            1.0062759188940589,
        ],
    ),
    (
        "synthetic-cfr-river-dry-5-checked",
        0.930_951_517_141_935_9,
        1.073207348649492,
        1.0416624644672752,
        [
            1.0378233110439326,
            0.984_554_376_181_505_5,
            1.0378233110439326,
            0.930_951_517_141_935_9,
            1.0378233110439326,
            1.0378233110439326,
            1.073207348649492,
            1.073207348649492,
            1.073207348649492,
            1.073207348649492,
            0.984_554_376_181_505_5,
            1.0062759188940589,
        ],
    ),
    (
        "synthetic-cfr-river-dry-6-checked",
        0.930_951_517_141_935_9,
        1.073207348649492,
        1.0025458611039653,
        [
            0.930_951_517_141_935_9,
            0.984_554_376_181_505_5,
            0.984_554_376_181_505_5,
            0.984_554_376_181_505_5,
            0.930_951_517_141_935_9,
            0.984_554_376_181_505_5,
            1.073207348649492,
            0.984_554_376_181_505_5,
            0.992768055410024,
            1.0378233110439326,
            1.073207348649492,
            1.073207348649492,
        ],
    ),
];

const SPOT_INDICES: [usize; 12] = [0, 1, 37, 100, 250, 500, 700, 900, 1000, 1050, 1070, 1080];

#[test]
fn reconstructed_public_range_weights_match_js() {
    let records = load_river_records();
    let mut checked = 0usize;
    let mut uniform = 0usize;
    for record in &records {
        let reconstructed = reconstruct(record);
        let weights = &reconstructed.request.weights;
        assert_eq!(weights.len(), 1081, "{}: combination count", record.id);
        for weight in weights {
            assert_eq!(
                weight.0, 1.0,
                "{}: hero range is the uniform fresh belief",
                record.id
            );
        }
        let summary = CHECKED_WEIGHT_SUMMARY
            .iter()
            .find(|(id, ..)| *id == record.id);
        match summary {
            Some((_, expected_min, expected_max, expected_mean, expected_spots)) => {
                checked += 1;
                let opponent: Vec<f64> = weights.iter().map(|weight| weight.1).collect();
                let min = opponent.iter().copied().fold(f64::INFINITY, f64::min);
                let max = opponent.iter().copied().fold(f64::NEG_INFINITY, f64::max);
                let mean = opponent.iter().sum::<f64>() / opponent.len() as f64;
                assert!(
                    (min - expected_min).abs() <= 1e-12
                        && (max - expected_max).abs() <= 1e-12
                        && (mean - expected_mean).abs() <= 1e-12,
                    "{}: weight summary ({min}, {max}, {mean}) != ({expected_min}, {expected_max}, {expected_mean})",
                    record.id
                );
                for (slot, index) in SPOT_INDICES.iter().enumerate() {
                    assert!(
                        (opponent[*index] - expected_spots[slot]).abs() <= 1e-12,
                        "{}: weight[{index}] {} != {}",
                        record.id,
                        opponent[*index],
                        expected_spots[slot]
                    );
                }
            }
            None => {
                uniform += 1;
                for (index, weight) in weights.iter().enumerate() {
                    assert_eq!(
                        weight.1, 1.0,
                        "{}: uniform opponent weight at {index}",
                        record.id
                    );
                }
            }
        }
    }
    assert_eq!(
        checked, 6,
        "six checked records with belief-weighted opponent ranges"
    );
    assert_eq!(uniform, 7, "seven records with uniform public ranges");
}

// ---------------------------------------------------------------------------
// Range abstraction self-tests
// ---------------------------------------------------------------------------

fn standard_board() -> [Card; 5] {
    ["Kc", "8d", "3h", "2s", "Jc"]
        .iter()
        .map(|text| text.parse().expect("board card"))
        .collect::<Vec<Card>>()
        .try_into()
        .expect("five board cards")
}

#[test]
fn range_abstraction_structure_and_validation() {
    let deck = Deck::standard();
    let board = standard_board();
    let combinations = 47 * 46 / 2;
    let uniform = vec![(1.0, 1.0); combinations];
    let abstraction = ranges::ranges(&board, deck.cards(), &uniform).expect("uniform ranges");
    assert_eq!(abstraction.combinations, 1081);
    assert_eq!(abstraction.combos.len(), 1081);
    assert_eq!(
        abstraction.deals.len(),
        36,
        "all 36 bucket cells are reachable"
    );
    let total: f64 = abstraction.deals.iter().map(|deal| deal.weight).sum();
    assert!(
        (total - 1.0).abs() < 1e-12,
        "deal weights sum to one: {total}"
    );
    for deal in &abstraction.deals {
        assert!(deal.weight > 0.0 && deal.weight <= 1.0);
        assert!((-1.0..=1.0).contains(&deal.showdown_sign));
        assert!(deal.scores[0] < 6 && deal.scores[1] < 6);
    }

    // Equal scores are never split across buckets and bucket ids rise with score.
    let mut score_buckets: BTreeMap<u64, BTreeSet<u8>> = BTreeMap::new();
    for combo in &abstraction.combos {
        score_buckets
            .entry(combo.score)
            .or_default()
            .insert(combo.bucket);
    }
    for (score, buckets) in &score_buckets {
        assert_eq!(buckets.len(), 1, "score {score} split across buckets");
    }
    assert!(
        score_buckets.len() > 6,
        "river board has many distinct scores"
    );
    assert_eq!(abstraction.bucket_by_score.len(), score_buckets.len());
    let mut previous_bucket = 0u8;
    for (score, buckets) in &score_buckets {
        let bucket = *buckets.iter().next().expect("one bucket");
        assert!(
            bucket >= previous_bucket,
            "bucket order regresses at score {score}"
        );
        assert_eq!(abstraction.bucket_by_score.get(score), Some(&bucket));
        previous_bucket = bucket;
    }

    // Exact card removal: a two-card available deck can never form a deal.
    let mut tiny_deck = vec![board[0], board[1], board[2], board[3], board[4]];
    tiny_deck.push("Ah".parse().expect("card"));
    tiny_deck.push("Ad".parse().expect("card"));
    let error = ranges::ranges(&board, &tiny_deck, &[(1.0, 1.0)]).expect_err("no joint deal");
    assert_eq!(error, AiError::RiverInvalid("no compatible private deals"));

    // Validation mirrors the original JS errors.
    let duplicate_board = [board[0], board[0], board[2], board[3], board[4]];
    assert_eq!(
        ranges::ranges(&duplicate_board, deck.cards(), &uniform).unwrap_err(),
        AiError::RiverInvalid("five distinct public cards required")
    );
    assert_eq!(
        ranges::ranges(&board, deck.cards(), &uniform[..10]).unwrap_err(),
        AiError::RiverInvalid("public range weight count mismatch")
    );
    let mut negative = uniform.clone();
    negative[0] = (-1.0, 1.0);
    assert_eq!(
        ranges::ranges(&board, deck.cards(), &negative).unwrap_err(),
        AiError::RiverInvalid("invalid public range")
    );
    let mut infinite = uniform.clone();
    infinite[0] = (f64::INFINITY, 1.0);
    assert_eq!(
        ranges::ranges(&board, deck.cards(), &infinite).unwrap_err(),
        AiError::RiverInvalid("invalid public range")
    );
    let empty = vec![(0.0, 0.0); combinations];
    assert_eq!(
        ranges::ranges(&board, deck.cards(), &empty).unwrap_err(),
        AiError::RiverInvalid("empty range")
    );
}

// ---------------------------------------------------------------------------
// Finite tree self-tests
// ---------------------------------------------------------------------------

fn uniform_deals() -> Vec<ranges::Deal> {
    let deck = Deck::standard();
    let board = standard_board();
    let uniform = vec![(1.0, 1.0); 1081];
    ranges::ranges(&board, deck.cards(), &uniform)
        .expect("uniform ranges")
        .deals
}

#[test]
fn finite_river_tree_menus_and_payoffs() {
    let deals = uniform_deals();
    // Observed check: the hero is in position and the villain already checked.
    let checked = RiverGame::new(
        700.0,
        [0.0, 0.0],
        [4650.0, 4650.0],
        0.0,
        200.0,
        200.0,
        100.0,
        true,
        true,
        deals.clone(),
    )
    .expect("checked game");
    let checked_root = checked.initial_states()[0].0.clone();
    assert_eq!(
        checked.actions(&checked_root),
        vec!["check", "bet:200", "bet:500", "bet:700", "bet:4650"]
    );
    assert_eq!(
        checked.key(&checked_root, 0),
        format!("0:{}:", checked_root.scores[0])
    );
    // Hero check closes the street immediately (the villain check is already in).
    let showdown = checked.child(&checked_root, "check");
    assert!(checked.terminal(&showdown));
    assert_eq!(showdown.end, Some(End::Showdown));
    assert_eq!(
        checked.payoff(&showdown),
        showdown.sign * (checked.base / 2.0 + checked_root.paid[0])
    );

    // Hero acts first: a check does not end the street.
    let first = RiverGame::new(
        700.0,
        [0.0, 0.0],
        [4650.0, 4650.0],
        0.0,
        200.0,
        200.0,
        100.0,
        false,
        true,
        deals.clone(),
    )
    .expect("first game");
    let first_root = first.initial_states()[0].0.clone();
    let villain_node = first.child(&first_root, "check");
    assert!(!first.terminal(&villain_node));
    assert_eq!(first.player(&villain_node), 1);
    assert_eq!(
        first.actions(&villain_node),
        vec!["check", "bet:200", "bet:500", "bet:700", "bet:4650"]
    );
    let checked_showdown = first.child(&villain_node, "check");
    assert_eq!(checked_showdown.end, Some(End::Showdown));
    // Hero bet, villain folds: the hero wins half the pre-street pot (his own bet
    // is refunded by the betting engine).
    let bet_node = first.child(&first_root, "bet:200");
    assert_eq!(first.player(&bet_node), 1);
    assert_eq!(
        first.actions(&bet_node),
        vec![
            "fold",
            "call",
            "raise:600",
            "raise:900",
            "raise:1300",
            "raise:4650"
        ]
    );
    let fold_node = first.child(&bet_node, "fold");
    assert_eq!(fold_node.end, Some(End::Fold { winner: 0 }));
    assert_eq!(
        first.payoff(&fold_node),
        first.base / 2.0 + fold_node.paid[1]
    );
    let call_node = first.child(&bet_node, "call");
    assert_eq!(call_node.end, Some(End::Showdown));
    assert_eq!(call_node.paid, [200.0, 200.0]);
    assert_eq!(
        first.payoff(&call_node),
        call_node.sign * (first.base / 2.0 + call_node.paid[0])
    );
    // Villain raise after hero bet: at most one future raise, so no re-raise.
    let villain_raise = first.child(&bet_node, "raise:700");
    assert_eq!(first.actions(&villain_raise), vec!["fold", "call"]);

    // Facing a river bet: fold/call/raise menu with the effective all-in.
    let facing = RiverGame::new(
        1000.0,
        [0.0, 300.0],
        [4350.0, 4350.0],
        300.0,
        500.0,
        200.0,
        100.0,
        false,
        true,
        deals,
    )
    .expect("facing game");
    let facing_root = facing.initial_states()[0].0.clone();
    assert_eq!(
        facing.actions(&facing_root),
        vec![
            "fold",
            "call",
            "raise:700",
            "raise:1200",
            "raise:1600",
            "raise:4350"
        ]
    );
    assert_eq!(
        facing.key(&facing_root, 0),
        format!("0:{}:", facing_root.scores[0])
    );
    let fold = facing.child(&facing_root, "fold");
    assert_eq!(fold.end, Some(End::Fold { winner: 1 }));
    assert_eq!(facing.payoff(&fold), -(facing.base / 2.0 + fold.paid[0]));
    let call = facing.child(&facing_root, "call");
    assert_eq!(call.paid, [300.0, 300.0]);
    assert_eq!(call.end, Some(End::Showdown));

    // Zero-sum check on a terminal fold: hero payoff is the negative of the
    // villain payoff when the winner flips.
    let hero_wins = State {
        end: Some(End::Fold { winner: 0 }),
        ..first_root.clone()
    };
    let villain_wins = State {
        end: Some(End::Fold { winner: 1 }),
        ..first_root
    };
    assert!((first.payoff(&hero_wins) + first.payoff(&villain_wins)).abs() < 1e-12);
    assert_eq!(
        first.payoff(&hero_wins),
        first.base / 2.0 + hero_wins.paid[1]
    );

    // Unsupported geometry is rejected like the original JS game().
    assert_eq!(
        RiverGame::new(
            100.0,
            [0.0, 0.0],
            [0.0, 100.0],
            0.0,
            200.0,
            200.0,
            100.0,
            false,
            true,
            Vec::new()
        )
        .unwrap_err(),
        AiError::RiverInvalid("unsupported river pot or stacks")
    );
    assert_eq!(
        RiverGame::new(
            100.0,
            [10.0, 0.0],
            [1000.0, 1000.0],
            0.0,
            200.0,
            200.0,
            100.0,
            false,
            true,
            Vec::new()
        )
        .unwrap_err(),
        AiError::RiverInvalid("unsupported river pot or stacks")
    );
}

// ---------------------------------------------------------------------------
// Kuhn poker: equilibrium and independent recursive oracle
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct KuhnState {
    cards: [u8; 2],
    history: Vec<String>,
}

impl KuhnState {
    fn terminal(&self) -> bool {
        match self.history.len() {
            0 | 1 => false,
            2 => {
                (self.history[0] == "check" && self.history[1] == "check")
                    || (self.history[0] == "bet"
                        && (self.history[1] == "fold" || self.history[1] == "call"))
            }
            3 => {
                self.history[0] == "check"
                    && self.history[1] == "bet"
                    && (self.history[2] == "fold" || self.history[2] == "call")
            }
            _ => true,
        }
    }

    fn actions(&self) -> Vec<String> {
        match self.history.len() {
            0 => vec!["check".to_owned(), "bet".to_owned()],
            1 => {
                if self.history[0] == "check" {
                    vec!["check".to_owned(), "bet".to_owned()]
                } else {
                    vec!["fold".to_owned(), "call".to_owned()]
                }
            }
            2 => vec!["fold".to_owned(), "call".to_owned()],
            _ => Vec::new(),
        }
    }

    fn sign(&self) -> f64 {
        if self.cards[0] > self.cards[1] {
            1.0
        } else {
            -1.0
        }
    }

    fn payoff(&self) -> f64 {
        match self.history.as_slice() {
            [first, second] if first == "check" && second == "check" => self.sign(),
            [first, second] if first == "bet" && second == "call" => 2.0 * self.sign(),
            [first, second, third] if first == "check" && second == "bet" && third == "call" => {
                2.0 * self.sign()
            }
            [first, second] if first == "bet" && second == "fold" => 1.0,
            [first, second, third] if first == "check" && second == "bet" && third == "fold" => {
                -1.0
            }
            _ => panic!("non-terminal Kuhn state asked for a payoff"),
        }
    }
}

struct KuhnGame;

impl GameTree for KuhnGame {
    type State = KuhnState;

    fn roots(&self) -> Vec<(KuhnState, f64)> {
        let mut roots = Vec::with_capacity(6);
        for first in 0u8..3 {
            for second in 0u8..3 {
                if first == second {
                    continue;
                }
                roots.push((
                    KuhnState {
                        cards: [first, second],
                        history: Vec::new(),
                    },
                    1.0 / 6.0,
                ));
            }
        }
        roots
    }

    fn terminal(&self, state: &KuhnState) -> bool {
        state.terminal()
    }

    fn player(&self, state: &KuhnState) -> u8 {
        u8::try_from(state.history.len() % 2).expect("Kuhn actor is 0 or 1")
    }

    fn key(&self, state: &KuhnState, player: u8) -> String {
        format!(
            "{}:{}:{}",
            player,
            state.cards[usize::from(player)],
            state.history.join("|")
        )
    }

    fn actions(&self, state: &KuhnState) -> Vec<String> {
        state.actions()
    }

    fn child(&self, state: &KuhnState, action: &str) -> KuhnState {
        let mut next = state.clone();
        next.history.push(action.to_owned());
        next
    }

    fn payoff(&self, state: &KuhnState) -> f64 {
        state.payoff()
    }
}

/// Expected payoff of player 0 when both players follow the average policy.
fn average_policy_value<G: GameTree>(
    game: &G,
    policy: &BTreeMap<String, RiverPolicyNode>,
    state: &G::State,
) -> f64 {
    if game.terminal(state) {
        return game.payoff(state);
    }
    let player = game.player(state);
    let key = game.key(state, player);
    let node = policy
        .get(&key)
        .expect("policy covers every information set");
    let mut value = 0.0f64;
    for (index, action) in node.actions.iter().enumerate() {
        value += node.probabilities[index]
            * average_policy_value(game, policy, &game.child(state, action));
    }
    value
}

fn average_policy_game_value<G: GameTree>(
    game: &G,
    policy: &BTreeMap<String, RiverPolicyNode>,
) -> f64 {
    let mut value = 0.0f64;
    for (state, probability) in game.roots() {
        value += probability * average_policy_value(game, policy, &state);
    }
    value
}

#[test]
fn kuhn_poker_reaches_equilibrium_value() {
    let game = KuhnGame;
    let mut compiled = CompiledCfr::compile(&game).expect("compile Kuhn tree");
    assert_eq!(compiled.infos.len(), 12, "Kuhn has 12 information sets");
    // CompiledCfr::run keeps the original 1..=100000 validation, so drive the
    // iteration step directly for the longer convergence run.
    for _ in 0..200_000u32 {
        compiled.iteration();
    }
    let policy = compiled.policy();
    for node in policy.values() {
        let sum: f64 = node.probabilities.iter().sum();
        assert!(
            (sum - 1.0).abs() < 1e-9,
            "policy probabilities sum to {sum}"
        );
    }
    let value = average_policy_game_value(&game, &policy);
    eprintln!("Kuhn average-policy value for player 0: {value:.9} (target -1/18)");
    let target = -1.0 / 18.0;
    assert!(
        (value - target).abs() < 3e-3,
        "Kuhn value {value} not within 3e-3 of {target}"
    );

    // The production iteration count must already be close to the equilibrium.
    let mut short = CompiledCfr::compile(&game).expect("compile Kuhn tree");
    short.run(800).expect("run 800 iterations");
    let short_value = average_policy_game_value(&game, &short.policy());
    assert!(
        (short_value - target).abs() < 0.1,
        "800-iteration Kuhn value {short_value} is implausible"
    );
}

/// Independent recursive tabular CFR used as an oracle for the compiled tables.
struct OracleInfo {
    actions: Vec<String>,
    regrets: Vec<f64>,
    average: Vec<f64>,
}

fn oracle_sigma(info: &OracleInfo) -> Vec<f64> {
    let sum: f64 = info.regrets.iter().map(|regret| regret.max(0.0)).sum();
    info.regrets
        .iter()
        .map(|regret| {
            if sum != 0.0 {
                regret.max(0.0) / sum
            } else {
                1.0 / info.actions.len() as f64
            }
        })
        .collect()
}

fn oracle_pass<G: GameTree>(
    game: &G,
    state: &G::State,
    r0: f64,
    r1: f64,
    chance: f64,
    infos: &mut BTreeMap<String, OracleInfo>,
    deltas: &mut BTreeMap<String, Vec<f64>>,
) -> f64 {
    if game.terminal(state) {
        return game.payoff(state);
    }
    let player = game.player(state);
    let key = game.key(state, player);
    let actions = game.actions(state);
    let entry = infos.entry(key.clone()).or_insert_with(|| OracleInfo {
        actions: actions.clone(),
        regrets: vec![0.0f64; actions.len()],
        average: vec![0.0f64; actions.len()],
    });
    assert_eq!(
        entry.actions, actions,
        "inconsistent oracle information set"
    );
    let sigma = oracle_sigma(entry);
    let reach = if player == 0 { r0 } else { r1 };
    for (index, probability) in sigma.iter().enumerate() {
        entry.average[index] += (chance * reach) * probability;
    }
    let mut child_values = Vec::with_capacity(actions.len());
    for (index, action) in actions.iter().enumerate() {
        let probability = sigma[index];
        let child = game.child(state, action);
        let child_r0 = if player == 0 { r0 * probability } else { r0 };
        let child_r1 = if player == 1 { r1 * probability } else { r1 };
        child_values.push(oracle_pass(
            game, &child, child_r0, child_r1, chance, infos, deltas,
        ));
    }
    let mut value = 0.0f64;
    for (index, probability) in sigma.iter().enumerate() {
        value += probability * child_values[index];
    }
    let weight = chance * if player == 0 { r1 } else { -r0 };
    let delta = deltas
        .entry(key)
        .or_insert_with(|| vec![0.0f64; actions.len()]);
    for (index, child_value) in child_values.iter().enumerate() {
        delta[index] += weight * (child_value - value);
    }
    value
}

fn recursive_cfr<G: GameTree>(game: &G, iterations: u32) -> BTreeMap<String, Vec<f64>> {
    let mut infos: BTreeMap<String, OracleInfo> = BTreeMap::new();
    for _ in 0..iterations {
        let mut deltas: BTreeMap<String, Vec<f64>> = BTreeMap::new();
        for (state, probability) in game.roots() {
            oracle_pass(game, &state, 1.0, 1.0, probability, &mut infos, &mut deltas);
        }
        for (key, delta) in deltas {
            let info = infos.get_mut(&key).expect("oracle information set exists");
            for (index, value) in delta.iter().enumerate() {
                info.regrets[index] += value;
            }
        }
    }
    infos
        .into_iter()
        .map(|(key, info)| {
            let sum: f64 = info.average.iter().sum();
            let probabilities = if sum != 0.0 {
                info.average.iter().map(|value| value / sum).collect()
            } else {
                vec![1.0 / info.actions.len() as f64; info.actions.len()]
            };
            (key, probabilities)
        })
        .collect()
}

#[test]
fn compiled_cfr_matches_recursive_oracle() {
    let game = KuhnGame;
    let iterations = 500u32;
    let mut compiled = CompiledCfr::compile(&game).expect("compile Kuhn tree");
    compiled.run(iterations).expect("run compiled CFR");
    let compiled_policy = compiled.policy();
    let oracle_policy = recursive_cfr(&game, iterations);
    assert_eq!(
        compiled_policy.len(),
        oracle_policy.len(),
        "same information sets"
    );
    let mut max_diff = 0.0f64;
    for (key, probabilities) in &compiled_policy {
        let oracle = oracle_policy
            .get(key)
            .unwrap_or_else(|| panic!("oracle misses {key}"));
        max_diff = max_diff.max(max_abs_diff(&probabilities.probabilities, oracle));
    }
    eprintln!("compiled vs recursive CFR policy max diff: {max_diff:e}");
    assert!(
        max_diff < 1e-9,
        "compiled and recursive CFR policies differ by {max_diff:e}"
    );
}

// ---------------------------------------------------------------------------
// select_decision contract
// ---------------------------------------------------------------------------

fn synthetic_solution(
    bucket: u8,
    actions: &[&str],
    probabilities: &[f64],
    action_values: &[f64],
) -> (RiverSolution, u64) {
    let key = format!("0:{bucket}:");
    let mut policy = BTreeMap::new();
    policy.insert(
        key.clone(),
        RiverPolicyNode {
            player: 0,
            actions: actions.iter().map(ToString::to_string).collect(),
            probabilities: probabilities.to_vec(),
        },
    );
    let mut root_values = BTreeMap::new();
    root_values.insert(key, action_values.to_vec());
    let mut bucket_by_score = BTreeMap::new();
    let score = 90_000_000_000u64;
    bucket_by_score.insert(score, bucket);
    let solution = RiverSolution {
        policy,
        root_values,
        bucket_by_score,
        iterations: 800,
        information_sets: 3,
        tree_nodes: 30,
        scope: SCOPE,
    };
    (solution, score)
}

#[test]
fn select_decision_contract() {
    let records = load_river_records();
    let base = reconstruct(record(&records, "synthetic-cfr-river-dry-1-checked"));
    let mut context = base.context.clone();

    // First action on roll 0, last action on a roll that exhausts the mass.
    let (solution, score) =
        synthetic_solution(5, &["check", "bet:200"], &[0.25, 0.75], &[10.0, 20.0]);
    let first = river::select_decision(&solution, &context, score, 0.0).expect("roll 0");
    assert_eq!(first.action, poker_engine::ActionType::Check);
    assert_eq!(first.amount, 0.0);
    assert!((first.ev - 10.0).abs() < 1e-12);
    let second = river::select_decision(&solution, &context, score, 0.5).expect("roll 0.5");
    assert_eq!(second.action, poker_engine::ActionType::Raise);
    assert_eq!(second.amount, 200.0);
    assert!((second.ev - 20.0).abs() < 1e-12);
    assert_eq!(second.trace.iterations, 800);
    assert_eq!(second.trace.information_sets, 3);
    assert_eq!(second.trace.tree_nodes, 30);
    assert_eq!(second.trace.bucket, 5);
    assert_eq!(second.trace.scope, SCOPE);

    // Odd effective stack: round the target UP to the wager-unit grid.
    let mut odd_context = context.clone();
    for player in &mut odd_context.public_players {
        if player.seat == odd_context.active_opponent_seats[0] {
            player.stack = 4630.0;
        }
    }
    let (solution, score) = synthetic_solution(5, &["check", "bet:4630"], &[0.0, 1.0], &[0.0, 1.0]);
    let decision = river::select_decision(&solution, &odd_context, score, 0.9).expect("round up");
    assert_eq!(decision.action, poker_engine::ActionType::Raise);
    assert_eq!(decision.amount, 4700.0);

    // Call amount comes from the context, never from the solver action string.
    context.to_call = 300.0;
    let (solution, score) = synthetic_solution(5, &["fold", "call"], &[0.0, 1.0], &[0.0, 5.0]);
    let decision = river::select_decision(&solution, &context, score, 0.9).expect("call");
    assert_eq!(decision.action, poker_engine::ActionType::Call);
    assert_eq!(decision.amount, 300.0);

    // Missing bucket / information set is the JS error path.
    let (solution, _score) = synthetic_solution(5, &["check"], &[1.0], &[0.0]);
    assert_eq!(
        river::select_decision(&solution, &context, 123_456_789, 0.1).unwrap_err(),
        AiError::RiverInvalid("river solver misses hero information set")
    );
    let (mut solution, score) = synthetic_solution(5, &["check"], &[1.0], &[0.0]);
    solution.policy.clear();
    assert_eq!(
        river::select_decision(&solution, &context, score, 0.1).unwrap_err(),
        AiError::RiverInvalid("river solver misses hero information set")
    );

    // Positional checked-before inference: BTN in position is true, SB is false.
    assert!(river::checked_before_from_position(&base.context));
    let mut out_of_position = base.context.clone();
    for player in &mut out_of_position.public_players {
        if player.seat == out_of_position.hero_seat {
            player.position_from_button = Some(1);
        } else if player.seat == out_of_position.active_opponent_seats[0] {
            player.position_from_button = Some(0);
        }
    }
    assert!(!river::checked_before_from_position(&out_of_position));
    out_of_position.current_bet = 300.0;
    assert!(!river::checked_before_from_position(&out_of_position));
}

// ---------------------------------------------------------------------------
// Timing budget for the production solve
// ---------------------------------------------------------------------------

#[test]
fn river_cfr_solve_timing_budget() {
    let records = load_river_records();
    let base = reconstruct(record(&records, "synthetic-cfr-river-dry-1-first"));
    // Warm up (also verifies the solve succeeds before timing).
    river::solve(&base.request).expect("warmup solve");
    let runs = 10;
    let mut samples = Vec::with_capacity(runs);
    for _ in 0..runs {
        let start = Instant::now();
        let solution = river::solve(&base.request).expect("timed solve");
        samples.push(start.elapsed().as_secs_f64() * 1000.0);
        assert_eq!(solution.information_sets, 204);
    }
    samples.sort_by(f64::total_cmp);
    let index = ((samples.len() as f64 * 0.95).ceil() as usize).saturating_sub(1);
    let p95 = samples[index.min(samples.len() - 1)];
    let median = samples[samples.len() / 2];
    eprintln!("river CFR solve: median={median:.2}ms p95={p95:.2}ms (204 infosets, 3564 nodes)");
    if cfg!(debug_assertions) {
        // Unoptimized debug builds are 10-20x slower than release; this bound only
        // guards against pathological regressions. The production budget is
        // enforced on the release profile below.
        assert!(p95 < 5_000.0, "debug solve P95 {p95:.2}ms exceeds 5s");
    } else {
        assert!(
            p95 < 50.0,
            "release solve P95 {p95:.2}ms exceeds the 50ms budget"
        );
    }
}
