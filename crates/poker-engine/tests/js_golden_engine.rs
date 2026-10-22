//! Replays the scripted golden hands exported from the original JS
//! implementation (server/server-game.js ServerPokerGame) and requires an
//! exact state-for-state match: legal action sets, chips, pots, refunds and
//! settlement results.
//!
//! Corpus: oracle/golden-engine.js -> tests/golden/engine/*.json

use std::fs;
use std::path::PathBuf;

use poker_core::{Card, Deck, Rank, Variant};
use poker_engine::{ActError, Action, Game, GameConfig, HandResult, LegalActions, Phase, Player};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Debug, Deserialize)]
struct ScenarioFile {
    scenario: ScenarioMeta,
    hands: Vec<HandRecord>,
}

#[derive(Debug, Deserialize)]
struct ScenarioMeta {
    name: String,
    #[serde(rename = "isShortDeck")]
    is_short_deck: bool,
    players: usize,
    stack: u64,
    #[serde(rename = "smallBlind")]
    small_blind: Option<u64>,
    #[serde(rename = "bigBlind")]
    big_blind: Option<u64>,
    ante: Option<u64>,
    #[serde(rename = "minBet")]
    min_bet: Option<u64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct HandRecord {
    hand_id: u64,
    started: bool,
    pre_stacks: Vec<u64>,
    deck: Vec<String>,
    start: Option<Value>,
    steps: Vec<StepRecord>,
    result: Option<Value>,
    #[serde(rename = "final")]
    final_state: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StepRecord {
    action: ActionRecord,
    legal_before: Value,
    before: Value,
    after: Value,
}

#[derive(Debug, Deserialize)]
struct ActionRecord {
    action: String,
    amount: u64,
}

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tests")
        .join("golden")
        .join("engine")
}

fn card_label(card: Card) -> String {
    let rank = match card.rank() {
        Rank::Ten => "T".to_string(),
        other => other.as_str().to_string(),
    };
    format!("{rank}{}", card.suit().code())
}

fn parse_card_text(text: &str) -> Card {
    text.parse()
        .unwrap_or_else(|error| panic!("bad golden card {text}: {error}"))
}

fn snapshot(game: &Game) -> Value {
    json!({
        "phase": game.phase().as_str(),
        "pot": game.pot(),
        "currentBet": game.current_bet(),
        "lastRaise": game.last_raise(),
        "hasFullBetThisRound": game.has_full_bet_this_round(),
        "dealerPos": game.dealer_pos(),
        "sbIdx": game.sb_idx(),
        "bbIdx": game.bb_idx(),
        "currentPlayerIdx": game.current_player_idx(),
        "preflopRaiseCount": game.preflop_raise_count(),
        "preflopAggressor": game.preflop_aggressor(),
        "lastAggressor": game.last_aggressor(),
        "lastAggressorStreet": game.last_aggressor_street().map(Phase::as_str),
        "community": game.community_cards().iter().copied().map(card_label).collect::<Vec<_>>(),
        "deckLen": game.deck_len(),
        "roundBets": game.round_bets().to_vec(),
        "players": game.players().iter().map(|player| json!({
            "stack": player.stack,
            "chipsInPot": player.chips_in_pot,
            "roundBet": player.round_bet,
            "folded": player.folded,
            "isAllIn": player.is_all_in,
            "dealtIn": player.dealt_in,
            "holeCards": player.hole_cards.iter().copied().map(card_label).collect::<Vec<_>>(),
            "lastAction": player.last_action.map(|last| json!({
                "action": last.action.as_str(),
                "amount": last.amount,
            })),
        })).collect::<Vec<_>>(),
    })
}

fn legal_json(legal: &LegalActions) -> Value {
    json!({
        "actions": legal.actions.iter().map(|action| action.as_str()).collect::<Vec<_>>(),
        "toCall": legal.to_call,
        "canRaise": legal.can_raise,
        "minRaiseTo": legal.min_raise_to,
        "maxRaiseTo": legal.max_raise_to,
        "wagerUnit": legal.wager_unit,
    })
}

fn result_json(result: &HandResult) -> Value {
    let winner_amounts: serde_json::Map<String, Value> = result
        .winner_amounts
        .iter()
        .map(|(id, amount)| (id.clone(), json!(amount)))
        .collect();
    json!({
        "reason": result.reason.as_str(),
        "isShortDeck": result.is_short_deck,
        "pot": result.pot,
        "handRank": result.hand_rank,
        "winners": result.winners.iter().map(|winner| winner.id.clone()).collect::<Vec<_>>(),
        "winnerAmounts": winner_amounts,
        "results": result.results.iter().map(|entry| json!({
            "id": entry.identity.id,
            "handRank": entry.hand_rank,
        })).collect::<Vec<_>>(),
    })
}

fn first_diff(actual: &Value, expected: &Value, path: &str) -> Option<String> {
    match (actual, expected) {
        (Value::Object(actual_map), Value::Object(expected_map)) => {
            let mut keys: Vec<&String> = actual_map.keys().chain(expected_map.keys()).collect();
            keys.sort();
            keys.dedup();
            for key in keys {
                let child_path = format!("{path}.{key}");
                match (actual_map.get(key), expected_map.get(key)) {
                    (Some(actual_value), Some(expected_value)) => {
                        if let Some(diff) = first_diff(actual_value, expected_value, &child_path) {
                            return Some(diff);
                        }
                    }
                    (Some(actual_value), None) => {
                        return Some(format!("{child_path}: rust only = {actual_value}"));
                    }
                    (None, Some(expected_value)) => {
                        return Some(format!("{child_path}: js only = {expected_value}"));
                    }
                    (None, None) => {}
                }
            }
            None
        }
        (Value::Array(actual_items), Value::Array(expected_items)) => {
            if actual_items.len() != expected_items.len() {
                return Some(format!(
                    "{path}.len: rust={} js={}",
                    actual_items.len(),
                    expected_items.len()
                ));
            }
            for (index, (actual_value, expected_value)) in
                actual_items.iter().zip(expected_items.iter()).enumerate()
            {
                if let Some(diff) =
                    first_diff(actual_value, expected_value, &format!("{path}[{index}]"))
                {
                    return Some(diff);
                }
            }
            None
        }
        _ => {
            if actual == expected {
                None
            } else {
                Some(format!("{path}: rust={actual} js={expected}"))
            }
        }
    }
}

fn assert_json_eq(actual: &Value, expected: &Value, context: &str) {
    if actual == expected {
        return;
    }
    let diff = first_diff(actual, expected, "$").unwrap_or_else(|| "unknown".to_string());
    panic!("golden mismatch: {context}\nfirst diff: {diff}");
}

fn parse_action(record: &ActionRecord) -> Action {
    match record.action.as_str() {
        "fold" => Action::Fold,
        "check" => Action::Check,
        "call" => Action::Call,
        "raise" => Action::Raise { to: record.amount },
        "allin" => Action::AllIn,
        other => panic!("unknown golden action {other}"),
    }
}

fn build_config(meta: &ScenarioMeta) -> GameConfig {
    let variant = if meta.is_short_deck {
        Variant::ShortDeck
    } else {
        Variant::Standard
    };
    GameConfig {
        variant,
        starting_stack: meta.stack,
        small_blind: meta.small_blind.unwrap_or(0),
        big_blind: meta.big_blind.unwrap_or(0),
        ante: meta.ante.unwrap_or(0),
        min_bet: meta.min_bet.unwrap_or(meta.big_blind.unwrap_or(0)),
        max_players: meta.players,
    }
}

fn build_players(meta: &ScenarioMeta) -> Vec<Player> {
    (0..meta.players)
        .map(|seat| {
            let id = if seat == 0 {
                "human".to_string()
            } else {
                format!("ai-{seat}")
            };
            let name = if seat == 0 {
                "You".to_string()
            } else {
                format!("AI{seat}")
            };
            Player::new(id, seat, name, meta.stack).with_human(seat == 0)
        })
        .collect()
}

fn replay_scenario(path: &PathBuf) {
    let text = fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    let data: ScenarioFile = serde_json::from_str(&text).expect("scenario json");
    let meta = &data.scenario;
    let mut game = Game::new(build_config(meta), build_players(meta));
    let mut replayed_hands = 0usize;
    let mut replayed_steps = 0usize;
    for hand in &data.hands {
        for (seat, stack) in hand.pre_stacks.iter().enumerate() {
            game.players_mut()[seat].stack = *stack;
        }
        let deck_cards: Vec<Card> = hand.deck.iter().map(|text| parse_card_text(text)).collect();
        let deck = Deck::from_cards(deck_cards).expect("golden deck has no duplicates");
        let dealer_override = if game.dealer_pos() < 0 {
            hand.start
                .as_ref()
                .and_then(|state| state.get("dealerPos"))
                .and_then(Value::as_i64)
                .map(|dealer| dealer as usize)
        } else {
            None
        };
        let started = game.start_hand_with_deck(deck, dealer_override);
        assert_eq!(started, hand.started, "{}: hand start flag", meta.name);
        if !started {
            continue;
        }
        assert_eq!(game.hand_id(), hand.hand_id, "{}: hand id", meta.name);
        let context = format!("{} hand {} start", meta.name, hand.hand_id);
        assert_json_eq(
            &snapshot(&game),
            hand.start.as_ref().expect("start state"),
            &context,
        );
        for (index, step) in hand.steps.iter().enumerate() {
            let seat = game.current_player_idx();
            assert!(
                seat >= 0,
                "{} hand {} step {index}: no current player",
                meta.name,
                hand.hand_id
            );
            let legal = game.legal_actions(seat as usize);
            let context = format!(
                "{} hand {} step {index} legal actions",
                meta.name, hand.hand_id
            );
            assert_json_eq(&legal_json(&legal), &step.legal_before, &context);
            let context = format!("{} hand {} step {index} pre-state", meta.name, hand.hand_id);
            assert_json_eq(&snapshot(&game), &step.before, &context);
            let action = parse_action(&step.action);
            let outcome: Result<_, ActError> = game.act(seat as usize, action);
            let context = format!(
                "{} hand {} step {index} act {:?}",
                meta.name, hand.hand_id, step.action
            );
            outcome.unwrap_or_else(|error| panic!("{context} rejected: {error}"));
            let context = format!(
                "{} hand {} step {index} post-state",
                meta.name, hand.hand_id
            );
            assert_json_eq(&snapshot(&game), &step.after, &context);
            replayed_steps += 1;
        }
        let expected_result = hand.result.as_ref().expect("hand result");
        let actual_result = result_json(game.last_hand_result().expect("settled hand"));
        let context = format!("{} hand {} result", meta.name, hand.hand_id);
        assert_json_eq(&actual_result, expected_result, &context);
        let context = format!("{} hand {} final state", meta.name, hand.hand_id);
        assert_json_eq(
            &snapshot(&game),
            hand.final_state.as_ref().expect("final state"),
            &context,
        );
        replayed_hands += 1;
    }
    assert!(
        replayed_hands + 1 >= data.hands.len(),
        "{}: hands replayed",
        meta.name
    );
    println!(
        "{}: {} hands, {} steps OK",
        meta.name, replayed_hands, replayed_steps
    );
}

#[test]
fn engine_matches_js_golden_corpus() {
    let dir = golden_dir();
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", dir.display()))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect();
    files.sort();
    assert!(
        !files.is_empty(),
        "no golden engine corpus found in {}",
        dir.display()
    );
    for file in &files {
        replay_scenario(file);
    }
}
