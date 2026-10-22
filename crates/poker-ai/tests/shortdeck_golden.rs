//! Golden replay of the frozen short-deck strategy core.
//!
//! Corpus: tests/golden/ai/decisions-shortdeck.jsonl (73 harvested decisions)
//! plus the nine short-deck records of tests/golden/ai/decisions-synthetic.jsonl
//! = 82 decisions (docs/specs/ai-strategy.md section 11).
//!
//! Replay protocol (docs/specs/ai-strategy-notes.md section 3): construct the
//! production adapter for the recorded profile source, apply the recorded AI
//! state, replay the observation prefix through record_opponent_action /
//! record_showdown, then call decide on the normalised context. The wall clock
//! is not involved: the corpus was generated with a virtual clock, so the fixed
//! sample schedule always completes.
//!
//! Hard assertions: action + amount must match exactly for at least 95% of the
//! decisions (initial target, section 11.4) and rngCalls must match for at
//! least 95% (the strongest signal that the RNG consumption order is right).
//! Numeric comparisons use the section 11.4 tolerances and failures are grouped
//! and printed with their unrounded values for diagnosis.

use std::fs;
use std::path::{Path, PathBuf};

use poker_ai::adapter::BrainImpl;
use poker_ai::input::{ObservedAction, RawDecisionContext, ShowdownResult, card_vec};
use poker_ai::shortdeck::{ShortDeckSnapshot, ShortDeckSnapshotCandidate, ShortDeckSnapshotEquity};
use poker_ai::trace::DecisionTrace;
use poker_ai::{AIPlayer, DecisionWithTrace, ProfileSource, Style};
use poker_core::{Card, Variant};
use serde::Deserialize;
use serde_json::Value;

/// Relative path of the golden AI corpus from the crate manifest directory.
const CORPUS_DIR: &str = "../../tests/golden/ai";

/// One harvested short-deck decision, decoded from the JSONL record.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoldenRecord {
    id: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    tags: Vec<String>,
    ai: AiDescriptor,
    ai_state: AiState,
    #[serde(default)]
    observes: Vec<ObserveOp>,
    context: RawDecisionContext,
    expect: Expectation,
}

/// ai block: adapter class, personality and profile source.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AiDescriptor {
    #[serde(rename = "type", default)]
    ai_type: String,
    #[serde(default)]
    variant: String,
    style: String,
    #[serde(default)]
    seat_id: Option<usize>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    profile_source: String,
    #[serde(default)]
    starting_stack: Option<f64>,
}

/// aiState block: state applied to the adapter before deciding.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AiState {
    #[serde(default)]
    name: Option<String>,
    #[serde(default, deserialize_with = "card_vec::deserialize")]
    hole_cards: Vec<Card>,
    #[serde(default)]
    stack: f64,
    #[serde(default)]
    chips_in_pot: f64,
    #[serde(default)]
    position: Option<usize>,
    #[serde(default)]
    num_players: Option<usize>,
    #[serde(default)]
    big_blind: Option<f64>,
    #[serde(default)]
    is_short_deck: bool,
    #[serde(default)]
    starting_stack: Option<f64>,
}

/// One observation op in the prefix (observe or showdown).
#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
enum ObserveOp {
    /// recordOpponentAction event.
    Observe {
        /// Deserialised action event.
        event: ObservedAction,
    },
    /// recordShowdown event.
    Showdown {
        /// Seat that showed down.
        #[serde(rename = "seatId")]
        seat_id: usize,
        /// Showdown info.
        info: ShowdownResult,
    },
}

/// expect block: JS decision plus rounded trace and unrounded raw capture.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Expectation {
    action: String,
    amount: f64,
    rng_calls: u64,
    trace: Value,
    raw: Value,
}

/// Replay outcome for one record.
struct ReplayOutcome {
    decision: DecisionWithTrace,
    snapshot: Option<ShortDeckSnapshot>,
}

/// Counters for the aggregate report.
#[derive(Default)]
struct Stats {
    total: usize,
    action_ok: usize,
    amount_ok: usize,
    action_amount_ok: usize,
    rng_ok: usize,
    trace_ok: usize,
    raw_ok: usize,
    boundary: usize,
    max_equity_mean_diff: f64,
    max_ev_rel_diff: f64,
}

/// Reads and parses one JSONL file.
fn read_jsonl<T: for<'de> Deserialize<'de>>(path: &Path) -> Vec<T> {
    let text = fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    text.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            serde_json::from_str(line)
                .unwrap_or_else(|error| panic!("cannot parse {}: {error}", path.display()))
        })
        .collect()
}

/// Loads the 82 short-deck records in corpus order.
fn load_records() -> Vec<GoldenRecord> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(CORPUS_DIR);
    let mut records: Vec<GoldenRecord> = read_jsonl(&dir.join("decisions-shortdeck.jsonl"));
    let synthetic: Vec<GoldenRecord> = read_jsonl(&dir.join("decisions-synthetic.jsonl"));
    records.extend(
        synthetic
            .into_iter()
            .filter(|record| record.ai.variant == "shortdeck"),
    );
    records
}

/// Replays one recorded decision through the production adapter.
fn replay(record: &GoldenRecord) -> ReplayOutcome {
    let context = record.context.clone().normalize();
    assert_eq!(
        context.variant,
        Variant::ShortDeck,
        "{}: replay context is not short-deck",
        record.id
    );
    // Cross-check the adapter class against the profile source exactly like the
    // JS oracle: local short-deck tables use ShortDeckAIPlayer + SD_STYLE_CONFIGS,
    // online tables use AIPlayer + STYLE_CONFIGS.
    if record.ai.profile_source == "SD_STYLE_CONFIGS" {
        assert_eq!(record.ai.ai_type, "ShortDeckAIPlayer", "{}", record.id);
    } else {
        assert_eq!(record.ai.ai_type, "AIPlayer", "{}", record.id);
    }
    let style = Style::from_name(&record.ai.style)
        .unwrap_or_else(|| panic!("{}: unknown style {}", record.id, record.ai.style));
    let source = match record.ai.profile_source.as_str() {
        "SD_STYLE_CONFIGS" => ProfileSource::ShortDeckLocalStyle,
        _ => ProfileSource::ShortDeckServerStyle,
    };
    // The JS adapter derives the brain seat from currentPlayerIndex at decision
    // time; the synthetic corpus records omit ai.seatId entirely.
    let seat = record.ai.seat_id.unwrap_or(context.hero_seat);
    let starting_stack = record
        .ai
        .starting_stack
        .or(record.ai_state.starting_stack)
        .unwrap_or(0.0);
    let name = record
        .ai
        .name
        .clone()
        .or_else(|| record.ai_state.name.clone())
        .unwrap_or_default();
    let mut player = AIPlayer::new(
        &name,
        style,
        starting_stack,
        seat,
        Variant::ShortDeck,
        source,
    );
    player.set_hole_cards(&record.ai_state.hole_cards);
    player.stack = record.ai_state.stack;
    player.chips_in_pot = record.ai_state.chips_in_pot;
    player.position = record.ai_state.position.unwrap_or(seat);
    player.num_players = record.ai_state.num_players.unwrap_or(context.player_count);
    player.big_blind = record.ai_state.big_blind.unwrap_or(context.big_blind);
    player.is_short_deck = record.ai_state.is_short_deck;
    for op in &record.observes {
        match op {
            ObserveOp::Observe { event } => player.record_opponent_action(event),
            ObserveOp::Showdown { seat_id, info } => player.record_showdown(*seat_id, info),
        }
    }
    let decision = player.decide(&context);
    let snapshot = match &player.brain {
        BrainImpl::ShortDeck(brain) => brain.last_snapshot().cloned(),
        BrainImpl::Standard(_) => None,
    };
    ReplayOutcome { decision, snapshot }
}

// ---------------------------------------------------------------------------
// Comparison helpers
// ---------------------------------------------------------------------------

/// Absolute/relative float comparison.
fn close(got: f64, want: f64, abs_tol: f64, rel_tol: f64) -> bool {
    let diff = (got - want).abs();
    diff <= abs_tol.max(rel_tol * want.abs())
}

fn value_f64(value: &Value, key: &str) -> Option<f64> {
    value.get(key).and_then(Value::as_f64)
}

fn cmp_f64(
    errors: &mut Vec<String>,
    path: &str,
    got: f64,
    want: Option<f64>,
    abs_tol: f64,
    rel_tol: f64,
) {
    match want {
        None => errors.push(format!("{path}: expected value missing")),
        Some(want) => {
            if !close(got, want, abs_tol, rel_tol) {
                errors.push(format!("{path}: got {got:.12} want {want:.12}"));
            }
        }
    }
}

fn cmp_str(errors: &mut Vec<String>, path: &str, got: &str, want: Option<&Value>) {
    match want.and_then(Value::as_str) {
        None => errors.push(format!("{path}: expected string missing")),
        Some(want) if want != got => {
            errors.push(format!("{path}: got {got:?} want {want:?}"));
        }
        Some(_) => {}
    }
}

fn cmp_u64(errors: &mut Vec<String>, path: &str, got: u64, want: Option<&Value>) {
    match want.and_then(Value::as_u64) {
        None => errors.push(format!("{path}: expected integer missing")),
        Some(want) if want != got => {
            errors.push(format!("{path}: got {got} want {want}"));
        }
        Some(_) => {}
    }
}

fn cmp_bool(errors: &mut Vec<String>, path: &str, got: bool, want: Option<&Value>) {
    match want.and_then(Value::as_bool) {
        None => errors.push(format!("{path}: expected bool missing")),
        Some(want) if want != got => {
            errors.push(format!("{path}: got {got} want {want}"));
        }
        Some(_) => {}
    }
}

/// Compares the Rust trace against the rounded JS trace stored in the corpus.
fn check_trace(got: &DecisionTrace, want: &Value) -> Vec<String> {
    let mut errors = Vec::new();
    cmp_str(
        &mut errors,
        "strategyVersion",
        &got.strategy_version,
        want.get("strategyVersion"),
    );
    let params = &want["parameters"];
    cmp_str(
        &mut errors,
        "parameters.variant",
        &got.parameters.variant,
        params.get("variant"),
    );
    cmp_f64(
        &mut errors,
        "parameters.rangeEvidenceWeight",
        got.parameters.range_evidence_weight,
        value_f64(params, "rangeEvidenceWeight"),
        1e-12,
        1e-12,
    );
    cmp_f64(
        &mut errors,
        "parameters.calledRangeDiscount",
        got.parameters.called_range_discount,
        value_f64(params, "calledRangeDiscount"),
        1e-12,
        1e-12,
    );
    cmp_u64(&mut errors, "handId", got.hand_id, want.get("handId"));
    cmp_str(
        &mut errors,
        "decisionId",
        &got.decision_id,
        want.get("decisionId"),
    );
    cmp_str(&mut errors, "intent", &got.intent, want.get("intent"));
    let equity = &want["equity"];
    cmp_f64(
        &mut errors,
        "equity.mean",
        got.equity.mean,
        value_f64(equity, "mean"),
        1.1e-4,
        0.0,
    );
    cmp_f64(
        &mut errors,
        "equity.stdDev",
        got.equity.std_dev,
        value_f64(equity, "stdDev"),
        1.1e-4,
        0.0,
    );
    cmp_f64(
        &mut errors,
        "equity.confidence",
        got.equity.confidence,
        value_f64(equity, "confidence"),
        1.1e-4,
        0.0,
    );
    cmp_u64(
        &mut errors,
        "equity.samples",
        got.equity.samples,
        equity.get("samples"),
    );
    cmp_bool(
        &mut errors,
        "equity.timedOut",
        got.equity.timed_out,
        equity.get("timedOut"),
    );
    cmp_f64(
        &mut errors,
        "equity.standardError",
        got.equity.standard_error,
        value_f64(equity, "standardError"),
        1.1e-4,
        0.0,
    );
    cmp_f64(
        &mut errors,
        "equity.rangeConfidence",
        got.equity.range_confidence,
        value_f64(equity, "rangeConfidence"),
        1.1e-4,
        0.0,
    );
    cmp_bool(
        &mut errors,
        "equity.exact",
        got.equity.exact,
        equity.get("exact"),
    );
    cmp_f64(
        &mut errors,
        "potOdds",
        got.pot_odds,
        value_f64(want, "potOdds"),
        1.1e-4,
        0.0,
    );
    let image = &want["selfImage"];
    cmp_f64(
        &mut errors,
        "selfImage.tightness",
        got.self_image.tightness,
        value_f64(image, "tightness"),
        1e-9,
        0.0,
    );
    cmp_f64(
        &mut errors,
        "selfImage.aggression",
        got.self_image.aggression,
        value_f64(image, "aggression"),
        1e-9,
        0.0,
    );
    cmp_f64(
        &mut errors,
        "selfImage.riverAggression",
        got.self_image.river_aggression,
        value_f64(image, "riverAggression"),
        1e-9,
        0.0,
    );
    cmp_f64(
        &mut errors,
        "selfImage.shownBluffRate",
        got.self_image.shown_bluff_rate,
        value_f64(image, "shownBluffRate"),
        1e-9,
        0.0,
    );
    cmp_bool(
        &mut errors,
        "selfImage.recentlyBluffed",
        got.self_image.recently_bluffed,
        image.get("recentlyBluffed"),
    );
    compare_opponents(&mut errors, &got.opponents, &want["opponents"]);
    compare_trace_candidates(&mut errors, &got.candidates, &want["candidates"]);
    let chosen = &want["chosen"];
    cmp_str(
        &mut errors,
        "chosen.action",
        &got.chosen.action,
        chosen.get("action"),
    );
    cmp_u64(
        &mut errors,
        "chosen.amount",
        got.chosen.amount,
        chosen.get("amount"),
    );
    cmp_f64(
        &mut errors,
        "chosen.utility",
        got.chosen.utility,
        value_f64(chosen, "utility"),
        5.1e-3,
        0.0,
    );
    let plan = &want["nextPlan"];
    cmp_str(
        &mut errors,
        "nextPlan.id",
        &got.next_plan.id,
        plan.get("id"),
    );
    cmp_str(
        &mut errors,
        "nextPlan.intent",
        &got.next_plan.intent,
        plan.get("intent"),
    );
    cmp_u64(
        &mut errors,
        "nextPlan.maxRisk",
        got.next_plan.max_risk,
        plan.get("maxRisk"),
    );
    compare_string_list(
        &mut errors,
        "nextPlan.continueConditions",
        &got.next_plan.continue_conditions,
        plan.get("continueConditions"),
    );
    compare_string_list(
        &mut errors,
        "nextPlan.abortConditions",
        &got.next_plan.abort_conditions,
        plan.get("abortConditions"),
    );
    cmp_str(
        &mut errors,
        "nextPlan.responseToRaise",
        &got.next_plan.response_to_raise,
        plan.get("responseToRaise"),
    );
    errors
}

fn compare_string_list(errors: &mut Vec<String>, path: &str, got: &[String], want: Option<&Value>) {
    let Some(want) = want.and_then(Value::as_array) else {
        errors.push(format!("{path}: expected array missing"));
        return;
    };
    let want: Vec<&str> = want.iter().filter_map(Value::as_str).collect();
    if got.len() != want.len() || got.iter().zip(want.iter()).any(|(a, b)| a != b) {
        errors.push(format!("{path}: got {got:?} want {want:?}"));
    }
}

fn compare_opponents(
    errors: &mut Vec<String>,
    got: &[poker_ai::trace::TraceOpponent],
    want: &Value,
) {
    let Some(want) = want.as_array() else {
        errors.push("opponents: expected array missing".to_owned());
        return;
    };
    if got.len() != want.len() {
        errors.push(format!(
            "opponents: got {} entries want {}",
            got.len(),
            want.len()
        ));
        return;
    }
    for (index, (got, want)) in got.iter().zip(want.iter()).enumerate() {
        let base = format!("opponents[{index}]");
        cmp_u64(
            errors,
            &format!("{base}.seat"),
            got.seat as u64,
            want.get("seat"),
        );
        cmp_f64(
            errors,
            &format!("{base}.rangeWidth"),
            got.range_width,
            value_f64(want, "rangeWidth"),
            6e-4,
            0.0,
        );
        cmp_f64(
            errors,
            &format!("{base}.estimatedStrength"),
            got.estimated_strength,
            value_f64(want, "estimatedStrength"),
            6e-4,
            0.0,
        );
        cmp_f64(
            errors,
            &format!("{base}.confidence"),
            got.confidence,
            value_f64(want, "confidence"),
            6e-4,
            0.0,
        );
        cmp_bool(
            errors,
            &format!("{base}.foldsTooMuch"),
            got.folds_too_much,
            want.get("foldsTooMuch"),
        );
        cmp_bool(
            errors,
            &format!("{base}.callsTooMuch"),
            got.calls_too_much,
            want.get("callsTooMuch"),
        );
    }
}

fn compare_trace_candidates(
    errors: &mut Vec<String>,
    got: &[poker_ai::trace::TraceCandidate],
    want: &Value,
) {
    let Some(want) = want.as_array() else {
        errors.push("candidates: expected array missing".to_owned());
        return;
    };
    if got.len() != want.len() {
        errors.push(format!(
            "candidates: got {} entries want {}",
            got.len(),
            want.len()
        ));
        return;
    }
    for (index, (got, want)) in got.iter().zip(want.iter()).enumerate() {
        let base = format!("candidates[{index}]");
        cmp_str(
            errors,
            &format!("{base}.action"),
            &got.action,
            want.get("action"),
        );
        cmp_u64(
            errors,
            &format!("{base}.amount"),
            got.amount,
            want.get("amount"),
        );
        cmp_f64(
            errors,
            &format!("{base}.ev"),
            got.ev,
            value_f64(want, "ev"),
            5.1e-3,
            0.0,
        );
        cmp_f64(
            errors,
            &format!("{base}.risk"),
            got.risk,
            value_f64(want, "risk"),
            5.1e-3,
            0.0,
        );
        cmp_f64(
            errors,
            &format!("{base}.utility"),
            got.utility,
            value_f64(want, "utility"),
            5.1e-3,
            0.0,
        );
        cmp_str(
            errors,
            &format!("{base}.reason"),
            &got.reason,
            want.get("reason"),
        );
    }
}

/// Compares the unrounded snapshot against the oracle's raw capture.
fn check_raw(snapshot: Option<&ShortDeckSnapshot>, want: &Value) -> Vec<String> {
    let mut errors = Vec::new();
    let Some(snapshot) = snapshot else {
        errors.push("raw: no snapshot produced".to_owned());
        return errors;
    };
    cmp_bool(
        &mut errors,
        "raw.resolvedByRiverCfr",
        false,
        want.get("resolvedByRiverCfr"),
    );
    if let Some(equity) = want.get("equity").filter(|value| !value.is_null()) {
        compare_raw_equity(&mut errors, &snapshot.equity, equity);
    }
    if let Some(pre) = want.get("preLegalize").filter(|value| !value.is_null()) {
        match &snapshot.pre_legalize {
            None => errors.push("raw.preLegalize: expected value missing".to_owned()),
            Some(got) => {
                cmp_str(
                    &mut errors,
                    "raw.preLegalize.action",
                    &got.action,
                    pre.get("action"),
                );
                cmp_f64(
                    &mut errors,
                    "raw.preLegalize.amount",
                    got.amount,
                    value_f64(pre, "amount"),
                    1e-9,
                    0.0,
                );
            }
        }
    }
    if let Some(candidates) = want.get("candidates").and_then(Value::as_array) {
        if candidates.len() != snapshot.candidates.len() {
            errors.push(format!(
                "raw.candidates: got {} entries want {}",
                snapshot.candidates.len(),
                candidates.len()
            ));
        } else {
            for (index, (got, want)) in snapshot
                .candidates
                .iter()
                .zip(candidates.iter())
                .enumerate()
            {
                compare_raw_candidate(&mut errors, index, got, want);
            }
        }
    }
    errors
}

fn compare_raw_equity(errors: &mut Vec<String>, got: &ShortDeckSnapshotEquity, want: &Value) {
    cmp_f64(
        errors,
        "raw.equity.mean",
        got.mean,
        value_f64(want, "mean"),
        1e-9,
        1e-12,
    );
    cmp_f64(
        errors,
        "raw.equity.stdDev",
        got.std_dev,
        value_f64(want, "stdDev"),
        1e-9,
        1e-12,
    );
    cmp_f64(
        errors,
        "raw.equity.standardError",
        got.standard_error.unwrap_or(0.0),
        value_f64(want, "standardError"),
        1e-9,
        0.0,
    );
    cmp_f64(
        errors,
        "raw.equity.rangeConfidence",
        got.range_confidence.unwrap_or(0.0),
        value_f64(want, "rangeConfidence"),
        1e-12,
        0.0,
    );
    cmp_u64(
        errors,
        "raw.equity.samples",
        got.samples as u64,
        want.get("samples"),
    );
    cmp_f64(
        errors,
        "raw.equity.confidence",
        got.confidence,
        value_f64(want, "confidence"),
        1e-12,
        0.0,
    );
    cmp_bool(errors, "raw.equity.exact", got.exact, want.get("exact"));
    cmp_bool(
        errors,
        "raw.equity.timedOut",
        got.timed_out,
        want.get("timedOut"),
    );
    cmp_bool(
        errors,
        "raw.equity.stalled",
        got.stalled,
        want.get("stalled"),
    );
    match (want.get("subsetEquities"), &got.subset_equities) {
        (Some(Value::Null), None) => {}
        (Some(Value::Object(map)), Some(got_map)) => {
            if map.len() != got_map.len() {
                errors.push(format!(
                    "raw.equity.subsetEquities: got {} keys want {}",
                    got_map.len(),
                    map.len()
                ));
            }
            for (key, want_value) in map {
                match got_map.get(key) {
                    None => errors.push(format!("raw.equity.subsetEquities[{key}]: missing")),
                    Some(got_value) => cmp_f64(
                        errors,
                        &format!("raw.equity.subsetEquities[{key}]"),
                        *got_value,
                        want_value.as_f64(),
                        1e-12,
                        0.0,
                    ),
                }
            }
        }
        (Some(Value::Null), Some(got_map)) if got_map.is_empty() => {}
        (want_value, got_value) => errors.push(format!(
            "raw.equity.subsetEquities: shape mismatch got {got_value:?} want {want_value:?}"
        )),
    }
    if want
        .get("responseSamples")
        .is_some_and(|value| !value.is_null())
    {
        errors.push("raw.equity.responseSamples: unexpected non-null".to_owned());
    }
}

fn compare_raw_candidate(
    errors: &mut Vec<String>,
    index: usize,
    got: &ShortDeckSnapshotCandidate,
    want: &Value,
) {
    let base = format!("raw.candidates[{index}]");
    cmp_str(
        errors,
        &format!("{base}.action"),
        &got.action,
        want.get("action"),
    );
    cmp_f64(
        errors,
        &format!("{base}.amount"),
        got.amount,
        value_f64(want, "amount"),
        1e-9,
        0.0,
    );
    if want.get("invest").is_some() {
        cmp_f64(
            errors,
            &format!("{base}.invest"),
            got.invest,
            value_f64(want, "invest"),
            1e-9,
            0.0,
        );
    }
    cmp_f64(
        errors,
        &format!("{base}.ev"),
        got.ev,
        value_f64(want, "ev"),
        1e-6,
        1e-6,
    );
    if want.get("risk").is_some() {
        cmp_f64(
            errors,
            &format!("{base}.risk"),
            got.risk,
            value_f64(want, "risk"),
            1e-6,
            1e-6,
        );
    }
    cmp_f64(
        errors,
        &format!("{base}.utility"),
        got.utility,
        value_f64(want, "utility"),
        1e-6,
        1e-6,
    );
    cmp_str(
        errors,
        &format!("{base}.reason"),
        &got.reason,
        want.get("reason"),
    );
    if want
        .get("calledEquity")
        .is_some_and(|value| !value.is_null())
    {
        let expected = value_f64(want, "calledEquity");
        match got.called_equity {
            None => errors.push(format!("{base}.calledEquity: missing")),
            Some(called) => cmp_f64(
                errors,
                &format!("{base}.calledEquity"),
                called,
                expected,
                1e-9,
                1e-9,
            ),
        }
    }
    if want.get("planBonus").is_some() {
        cmp_f64(
            errors,
            &format!("{base}.planBonus"),
            got.plan_bonus,
            value_f64(want, "planBonus"),
            1e-9,
            0.0,
        );
    }
    if let Some(response) = want.get("response").filter(|value| !value.is_null()) {
        match &got.response {
            None => errors.push(format!("{base}.response: missing")),
            Some(got) => {
                let fields = [
                    ("allFold", got.all_fold),
                    ("anyCall", got.any_call),
                    ("anyRaise", got.any_raise),
                    ("expectedCallers", got.expected_callers),
                    ("credibility", got.credibility),
                    ("fraction", got.fraction),
                ];
                for (key, got_value) in fields {
                    cmp_f64(
                        errors,
                        &format!("{base}.response.{key}"),
                        got_value,
                        value_f64(response, key),
                        1e-9,
                        1e-9,
                    );
                }
            }
        }
    }
}

/// Boundary classification for an action mismatch: the top two utilities are
/// closer than the spec's 1e-6 mixing threshold (section 11.4).
fn is_boundary(snapshot: Option<&ShortDeckSnapshot>) -> bool {
    let Some(snapshot) = snapshot else {
        return false;
    };
    let mut utilities: Vec<f64> = snapshot
        .candidates
        .iter()
        .map(|candidate| candidate.utility)
        .collect();
    utilities.sort_by(|a, b| b.total_cmp(a));
    utilities.len() >= 2 && (utilities[0] - utilities[1]).abs() < 1e-6
}

/// Max relative difference of the EV fields (raw diagnostics).
fn max_ev_diff(snapshot: Option<&ShortDeckSnapshot>, want: &Value) -> f64 {
    let (Some(snapshot), Some(candidates)) =
        (snapshot, want.get("candidates").and_then(Value::as_array))
    else {
        return 0.0;
    };
    let mut worst: f64 = 0.0;
    for (got, want) in snapshot.candidates.iter().zip(candidates.iter()) {
        if let Some(want_ev) = value_f64(want, "ev") {
            let rel = (got.ev - want_ev).abs() / want_ev.abs().max(1.0);
            worst = worst.max(rel);
        }
    }
    worst
}

// ---------------------------------------------------------------------------
// The golden replay test
// ---------------------------------------------------------------------------

#[test]
fn golden_shortdeck_decisions_replay() {
    let records = load_records();
    assert_eq!(records.len(), 82, "expected 82 short-deck decisions");

    let mut stats = Stats::default();
    let mut failures: Vec<String> = Vec::new();
    let mut boundary_failures: Vec<String> = Vec::new();
    let mut failed_ids: Vec<String> = Vec::new();

    for record in &records {
        stats.total += 1;
        let outcome = replay(record);
        let expect = &record.expect;
        let action_ok = outcome.decision.action.as_str() == expect.action;
        let amount_ok = outcome.decision.amount == expect.amount as u64;
        let rng_ok = outcome.decision.trace.rng_calls == expect.rng_calls;
        let trace_errors = check_trace(&outcome.decision.trace, &expect.trace);
        let raw_errors = check_raw(outcome.snapshot.as_ref(), &expect.raw);

        if action_ok {
            stats.action_ok += 1;
        }
        if amount_ok {
            stats.amount_ok += 1;
        }
        if action_ok && amount_ok {
            stats.action_amount_ok += 1;
        }
        if rng_ok {
            stats.rng_ok += 1;
        }
        if trace_errors.is_empty() {
            stats.trace_ok += 1;
        }
        if raw_errors.is_empty() {
            stats.raw_ok += 1;
        }
        if let (Some(snapshot), Some(want_mean)) = (
            outcome.snapshot.as_ref(),
            value_f64(&expect.raw["equity"], "mean"),
        ) {
            stats.max_equity_mean_diff = stats
                .max_equity_mean_diff
                .max((snapshot.equity.mean - want_mean).abs());
        }
        stats.max_ev_rel_diff = stats
            .max_ev_rel_diff
            .max(max_ev_diff(outcome.snapshot.as_ref(), &expect.raw));

        if !(action_ok && amount_ok && rng_ok) {
            failed_ids.push(record.id.clone());
            let boundary = is_boundary(outcome.snapshot.as_ref());
            if boundary {
                stats.boundary += 1;
            }
            let detail = format!(
                "{} [{}] expected {} {} rng={} got {} {} rng={}{}{}",
                record.id,
                record.tags.join(","),
                expect.action,
                expect.amount as u64,
                expect.rng_calls,
                outcome.decision.action.as_str(),
                outcome.decision.amount,
                outcome.decision.trace.rng_calls,
                if boundary { " BOUNDARY" } else { "" },
                format_snapshot(outcome.snapshot.as_ref()),
            );
            if boundary {
                boundary_failures.push(detail);
            } else {
                failures.push(detail);
            }
        }
        if !trace_errors.is_empty() || !raw_errors.is_empty() {
            failed_ids.push(record.id.clone());
            let mut detail = format!("{} trace-errors={}", record.id, trace_errors.len());
            for error in trace_errors.iter().take(6) {
                detail.push_str(&format!("\n    trace {error}"));
            }
            for error in raw_errors.iter().take(8) {
                detail.push_str(&format!("\n    raw   {error}"));
            }
            failures.push(detail);
        }
    }

    let pct = |part: usize| 100.0 * part as f64 / stats.total as f64;
    println!(
        "=== short-deck golden replay: {} decisions ===",
        stats.total
    );
    println!(
        "action match           : {:3}/{} ({:.1}%)",
        stats.action_ok,
        stats.total,
        pct(stats.action_ok)
    );
    println!(
        "amount match           : {:3}/{} ({:.1}%)",
        stats.amount_ok,
        stats.total,
        pct(stats.amount_ok)
    );
    println!(
        "action+amount match    : {:3}/{} ({:.1}%)  [gate >= 95%]",
        stats.action_amount_ok,
        stats.total,
        pct(stats.action_amount_ok)
    );
    println!(
        "rngCalls match         : {:3}/{} ({:.1}%)  [gate >= 95%]",
        stats.rng_ok,
        stats.total,
        pct(stats.rng_ok)
    );
    println!(
        "trace within tolerance : {:3}/{} ({:.1}%)",
        stats.trace_ok,
        stats.total,
        pct(stats.trace_ok)
    );
    println!(
        "raw within tolerance   : {:3}/{} ({:.1}%)",
        stats.raw_ok,
        stats.total,
        pct(stats.raw_ok)
    );
    println!(
        "max raw equity.mean abs diff: {:.3e}",
        stats.max_equity_mean_diff
    );
    println!("max raw ev rel diff    : {:.3e}", stats.max_ev_rel_diff);
    if !boundary_failures.is_empty() {
        println!(
            "boundary-classified mismatches ({}):",
            boundary_failures.len()
        );
        for failure in boundary_failures.iter().take(10) {
            println!("  {failure}");
        }
    }
    if !failures.is_empty() {
        println!("failures ({}):", failures.len());
        for failure in failures.iter().take(20) {
            println!("  {failure}");
        }
        let mut by_tag: Vec<(String, usize)> = Vec::new();
        for record in &records {
            if failed_ids.iter().any(|id| id == &record.id) {
                for tag in &record.tags {
                    match by_tag.iter_mut().find(|(name, _)| name == tag) {
                        Some((_, count)) => *count += 1,
                        None => by_tag.push((tag.clone(), 1)),
                    }
                }
            }
        }
        by_tag.sort_by_key(|entry| std::cmp::Reverse(entry.1));
        println!("failure tags: {by_tag:?}");
    }

    assert!(
        pct(stats.action_amount_ok) >= 95.0,
        "action+amount exact rate {:.1}% is below the 95% initial target",
        pct(stats.action_amount_ok)
    );
    assert!(
        pct(stats.rng_ok) >= 95.0,
        "rngCalls exact rate {:.1}% is below the 95% initial target",
        pct(stats.rng_ok)
    );
    assert!(
        pct(stats.trace_ok) >= 99.0,
        "trace tolerance rate {:.1}% is below the 99% initial target",
        pct(stats.trace_ok)
    );
    assert!(
        pct(stats.raw_ok) >= 99.0,
        "raw tolerance rate {:.1}% is below the 99% initial target",
        pct(stats.raw_ok)
    );
}

/// Renders a short summary of the unrounded decision for failure reports.
fn format_snapshot(snapshot: Option<&ShortDeckSnapshot>) -> String {
    let Some(snapshot) = snapshot else {
        return String::new();
    };
    let mut lines = String::new();
    lines.push_str(&format!(
        "\n      raw equity mean={:.6} samples={} exact={} top_candidates=[",
        snapshot.equity.mean, snapshot.equity.samples, snapshot.equity.exact
    ));
    let mut candidates: Vec<&ShortDeckSnapshotCandidate> = snapshot.candidates.iter().collect();
    candidates.sort_by(|a, b| b.utility.total_cmp(&a.utility));
    for candidate in candidates.iter().take(3) {
        lines.push_str(&format!(
            "{}@{} u={:.3} ev={:.2}; ",
            candidate.action, candidate.amount, candidate.utility, candidate.ev
        ));
    }
    lines.push(']');
    lines
}

/// The corpus must contain exactly the short-deck decision records and the
/// profile-source split required by the coverage matrix.
#[test]
fn corpus_shape_matches_manifest() {
    let records = load_records();
    let local = records
        .iter()
        .filter(|record| record.ai.profile_source == "SD_STYLE_CONFIGS")
        .count();
    let server = records
        .iter()
        .filter(|record| record.ai.profile_source != "SD_STYLE_CONFIGS")
        .count();
    assert_eq!(local, 58, "short-deck local profile records");
    assert_eq!(server, 24, "short-deck server profile records");
    let harvested = records
        .iter()
        .filter(|record| record.category.starts_with("harvested"))
        .count();
    assert_eq!(harvested, 73);
    let synthetic = records
        .iter()
        .filter(|record| record.category == "synthetic")
        .count();
    assert_eq!(synthetic, 9);
}
