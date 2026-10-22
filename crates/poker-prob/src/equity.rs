//! Exact heads-up enumeration, fixed-seed Monte Carlo and the `calculate`
//! entry point.
//!
//! Port of `exactHeadsUp`, `monteCarlo` and `calculate` from
//! `js/probability-core.js`.

use std::collections::BTreeMap;
use std::fmt;
use std::time::Instant;

use poker_core::{Card, RandomSource, Variant, XorShift32Rng, evaluate};

use crate::ProbError;
use crate::improvements::{Improvements, find_improvements};
use crate::prepare::{Prepared, prepare};

const DEFAULT_SIMULATIONS: u32 = 2400;
const MIN_SIMULATIONS: u32 = 400;
const MAX_SIMULATIONS: u32 = 12_000;
const DEFAULT_MIN_SAMPLES: u32 = 700;
const MIN_MIN_SAMPLES: u32 = 350;

/// Which algorithm produced an [`EquityResult`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Method {
    /// Exhaustive enumeration for a heads-up turn or river.
    ExactHeadsUp,
    /// Fixed-seed Monte Carlo sampling.
    MonteCarlo,
}

impl Method {
    /// The method string used by the JS result and the golden corpus.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Method::ExactHeadsUp => "exact-heads-up",
            Method::MonteCarlo => "monte-carlo",
        }
    }
}

impl fmt::Display for Method {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Options for [`monte_carlo`] and [`calculate`].
///
/// The JS implementation clamps `simulations` and `minSamples` and can stop
/// early against a wall-clock `timeBudgetMs`. The Rust port replaces the time
/// budget with a deterministic target sample count: [`Self::target_samples`]
/// is always executed in full, so a fixed seed reproduces the exact same
/// samples. `min_samples` is kept for option parity and is clamped exactly like
/// the JS field, but the deterministic mode never consults it; the golden
/// generator passes `minSamples = target samples` so the JS reference does not
/// stop early either.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MonteCarloOptions {
    /// Seed string hashed exactly like the JS `hashSeed` helper. An empty
    /// string falls back to the default JS seed `"poker-probability"`.
    pub seed: String,
    /// Requested sample count. `0` falls back to the JS default `2400`; the
    /// effective target is clamped to `400..=12000` like the JS
    /// implementation.
    pub simulations: u32,
    /// JS `minSamples` value. It only gates the JS time-budget early exit, so
    /// it has no effect on deterministic Rust results. `0` falls back to the
    /// JS default `700` and the effective value is clamped to `350..=target`.
    pub min_samples: u32,
}

impl Default for MonteCarloOptions {
    fn default() -> Self {
        Self {
            seed: String::new(),
            simulations: DEFAULT_SIMULATIONS,
            min_samples: DEFAULT_MIN_SAMPLES,
        }
    }
}

impl MonteCarloOptions {
    /// Effective number of samples, clamped exactly like the JS implementation:
    /// `max(400, min(12000, simulations || 2400))`.
    #[must_use]
    pub fn target_samples(&self) -> u64 {
        let requested = if self.simulations == 0 {
            DEFAULT_SIMULATIONS
        } else {
            self.simulations
        };
        u64::from(requested.clamp(MIN_SIMULATIONS, MAX_SIMULATIONS))
    }

    /// Effective JS `minSamples`, clamped like
    /// `min(targetSamples, max(350, minSamples || 700))`.
    #[must_use]
    pub fn effective_min_samples(&self) -> u64 {
        let requested = if self.min_samples == 0 {
            DEFAULT_MIN_SAMPLES
        } else {
            self.min_samples
        };
        self.target_samples()
            .min(u64::from(requested.max(MIN_MIN_SAMPLES)))
    }
}

/// Result of an exact or Monte Carlo equity calculation.
///
/// Field names correspond to the JS result object: `winProb`, `tieProb`,
/// `loseProb`, `equityProb`, `samples`, `method`, `stats.bestHandCounts` and
/// the `outs` report that `calculate()` attaches. `elapsed_ms` is a wall-clock
/// diagnostic and is excluded from [`PartialEq`].
#[derive(Clone, Debug)]
pub struct EquityResult {
    /// Probability of winning outright, in `0..=1`.
    pub win_prob: f64,
    /// Probability of a tie at showdown, in `0..=1`.
    pub tie_prob: f64,
    /// Probability of losing at showdown, in `0..=1`.
    pub lose_prob: f64,
    /// Expected pot share in `0..=1` (ties are split by tied player count).
    pub equity_prob: f64,
    /// Number of enumerated or sampled showdowns.
    pub samples: u64,
    /// Which algorithm produced the result.
    pub method: Method,
    /// Wall-clock time spent, in milliseconds (diagnostic only).
    pub elapsed_ms: f64,
    /// Count of the player's final hand type across all samples
    /// (the JS `stats.bestHandCounts` object).
    pub best_hand_counts: BTreeMap<String, u64>,
    /// Outs report; populated by [`calculate`], `None` for direct
    /// [`exact_heads_up`] / [`monte_carlo`] calls.
    pub outs: Option<Improvements>,
}

impl PartialEq for EquityResult {
    fn eq(&self, other: &Self) -> bool {
        self.win_prob == other.win_prob
            && self.tie_prob == other.tie_prob
            && self.lose_prob == other.lose_prob
            && self.equity_prob == other.equity_prob
            && self.samples == other.samples
            && self.method == other.method
            && self.best_hand_counts == other.best_hand_counts
            && self.outs == other.outs
    }
}

#[derive(Clone, Copy, Debug)]
struct ShowdownResult {
    win: u64,
    tie: u64,
    loss: u64,
    equity: f64,
}

/// Mirrors `resolveShowdown(playerScore, opponentScores)` in the JS core.
fn resolve_showdown(player_score: u64, opponent_scores: &[u64]) -> ShowdownResult {
    let best = opponent_scores.iter().copied().max().unwrap_or(0);
    if player_score > best {
        return ShowdownResult {
            win: 1,
            tie: 0,
            loss: 0,
            equity: 1.0,
        };
    }
    if player_score < best {
        return ShowdownResult {
            win: 0,
            tie: 0,
            loss: 1,
            equity: 0.0,
        };
    }
    let tied = opponent_scores
        .iter()
        .filter(|&&score| score == player_score)
        .count() as u64;
    ShowdownResult {
        win: 0,
        tie: 1,
        loss: 0,
        equity: 1.0 / (tied as f64 + 1.0),
    }
}

/// Heads-up special case of [`resolve_showdown`].
fn resolve_heads_up(player_score: u64, opponent_score: u64) -> ShowdownResult {
    if player_score > opponent_score {
        ShowdownResult {
            win: 1,
            tie: 0,
            loss: 0,
            equity: 1.0,
        }
    } else if player_score < opponent_score {
        ShowdownResult {
            win: 0,
            tie: 0,
            loss: 1,
            equity: 0.0,
        }
    } else {
        ShowdownResult {
            win: 0,
            tie: 1,
            loss: 0,
            equity: 0.5,
        }
    }
}

#[derive(Default)]
struct Tally {
    wins: u64,
    ties: u64,
    losses: u64,
    equity: f64,
    samples: u64,
    best_hands: BTreeMap<&'static str, u64>,
}

impl Tally {
    fn add_showdown(&mut self, result: ShowdownResult) {
        self.wins += result.win;
        self.ties += result.tie;
        self.losses += result.loss;
        self.equity += result.equity;
        self.samples += 1;
    }

    fn record(&mut self, result: ShowdownResult, hand_name: &'static str) {
        self.add_showdown(result);
        *self.best_hands.entry(hand_name).or_insert(0) += 1;
    }

    fn finish(self, method: Method, started: Instant) -> EquityResult {
        let total = self.samples.max(1) as f64;
        EquityResult {
            win_prob: self.wins as f64 / total,
            tie_prob: self.ties as f64 / total,
            lose_prob: self.losses as f64 / total,
            equity_prob: self.equity / total,
            samples: self.samples,
            method,
            elapsed_ms: started.elapsed().as_secs_f64() * 1000.0,
            best_hand_counts: self
                .best_hands
                .into_iter()
                .map(|(name, count)| (name.to_string(), count))
                .collect(),
            outs: None,
        }
    }
}

/// Exhaustively enumerates a heads-up turn or river.
///
/// Mirrors `exactHeadsUp(prepared)` in `js/probability-core.js`:
///
/// * on a four-card board every river card is dealt and every remaining
///   opponent pair is enumerated, so the sample count is
///   `remaining * C(remaining - 1, 2)`;
/// * on a complete board only the `C(remaining, 2)` opponent pairs are
///   enumerated;
/// * ties split equity by the number of tied players (`1 / (tied + 1)`);
/// * `stats.bestHandCounts` counts the player's final hand name once per river
///   for `max(1, opponent pairs)` samples, exactly like the JS code.
///
/// A rayon-parallel river enumeration (merging integer tallies in river order)
/// was implemented and verified bit-identical, but it did not improve wall-clock
/// time on the WSL test machine (global pool oversubscription plus small tasks);
/// it was therefore not merged. See `docs/benchmarks.md` for the measurements.
///
/// # Errors
///
/// Returns [`ProbError::ExactRequiresTurnOrRiver`] when the prepared board has
/// fewer than four cards (the JS implementation is only called in the
/// four-or-five card case and has no guard for shorter boards).
pub fn exact_heads_up(prepared: &Prepared) -> Result<EquityResult, ProbError> {
    let started = Instant::now();
    let board = prepared.board();
    if board.len() < 4 {
        return Err(ProbError::ExactRequiresTurnOrRiver {
            board_cards: board.len(),
        });
    }
    let variant = prepared.variant();
    let player = prepared.player();
    let deck = prepared.remaining_deck();
    let missing = prepared.missing_board_cards();

    let mut hand = [player[0]; 7];
    hand[0] = player[0];
    hand[1] = player[1];
    hand[2..2 + board.len()].copy_from_slice(board);
    let mut tally = Tally::default();

    if missing == 1 {
        for river_index in 0..deck.len() {
            hand[0] = player[0];
            hand[1] = player[1];
            hand[2 + board.len()] = deck[river_index];
            let player_hand = evaluate(&hand, variant);
            let mut river_samples = 0u64;
            for first in 0..deck.len() - 1 {
                if first == river_index {
                    continue;
                }
                hand[0] = deck[first];
                for (second, &second_card) in deck.iter().enumerate().skip(first + 1) {
                    if second == river_index {
                        continue;
                    }
                    hand[1] = second_card;
                    let opponent = evaluate(&hand, variant);
                    tally.add_showdown(resolve_heads_up(player_hand.score(), opponent.score()));
                    river_samples += 1;
                }
            }
            *tally.best_hands.entry(player_hand.name()).or_insert(0) += river_samples.max(1);
        }
    } else {
        let player_hand = evaluate(&hand, variant);
        let mut river_samples = 0u64;
        for first in 0..deck.len() - 1 {
            hand[0] = deck[first];
            for &second_card in &deck[first + 1..] {
                hand[1] = second_card;
                let opponent = evaluate(&hand, variant);
                tally.add_showdown(resolve_heads_up(player_hand.score(), opponent.score()));
                river_samples += 1;
            }
        }
        *tally.best_hands.entry(player_hand.name()).or_insert(0) += river_samples.max(1);
    }

    Ok(tally.finish(Method::ExactHeadsUp, started))
}

/// Fixed-seed Monte Carlo equity against the prepared opponents.
///
/// Mirrors `monteCarlo(prepared, options)` in `js/probability-core.js`, with
/// one deliberate difference: there is no wall-clock `timeBudgetMs`, so the
/// loop always runs [`MonteCarloOptions::target_samples`] samples and a fixed
/// seed produces an exact JavaScript-parity sample stream:
///
/// * the RNG is [`XorShift32Rng`] seeded through the JS `hashSeed` FNV-1a
///   hash, which reproduces the JS `randomFactory` stream bit for bit;
/// * each sample performs the same partial Fisher-Yates draw
///   (`swap = index + floor(rand * (len - index))`) over a persistent pool and
///   then restores it in reverse order;
/// * the player hand is evaluated first, then one hand per opponent;
/// * the equity accumulator adds `1 / (tied + 1)` for ties in sample order.
pub fn monte_carlo(prepared: &Prepared, options: &MonteCarloOptions) -> EquityResult {
    let started = Instant::now();
    let variant = prepared.variant();
    let player = prepared.player();
    let board = prepared.board();
    let deck = prepared.remaining_deck();
    let opponents = prepared.opponents();
    let missing = prepared.missing_board_cards();
    let draw_count = missing + opponents * 2;
    let target_samples = options.target_samples();
    debug_assert!(options.effective_min_samples() <= target_samples);

    let mut pool = deck.to_vec();
    let mut swaps = vec![0usize; draw_count];
    let mut rng = XorShift32Rng::from_seed(&options.seed);
    let mut opponent_scores: Vec<u64> = Vec::with_capacity(opponents);
    let mut hand = [player[0]; 7];
    hand[0] = player[0];
    hand[1] = player[1];
    hand[2..2 + board.len()].copy_from_slice(board);
    let board_end = 2 + board.len();
    let mut tally = Tally::default();

    while tally.samples < target_samples {
        for (index, slot) in swaps.iter_mut().enumerate() {
            let span = pool.len() - index;
            // The JS reference draws `index + floor(rng() * span)` where
            // `rng() = state / 2^32`. Because `span <= 52`, the integer
            // identity `floor(u32 * span / 2^32) == (u32 * span) >> 32` holds
            // exactly (product < 2^38, no rounding can cross an integer
            // boundary), so this reproduces the JS sample stream bit for bit
            // while avoiding a per-sample f64 conversion.
            let draw = (u64::from(rng.next_u32()) * span as u64) >> 32;
            let swap = index + draw as usize;
            *slot = swap;
            pool.swap(index, swap);
        }
        for (offset, &card) in pool[..missing].iter().enumerate() {
            hand[board_end + offset] = card;
        }
        hand[0] = player[0];
        hand[1] = player[1];
        let player_hand = evaluate(&hand, variant);
        opponent_scores.clear();
        for opponent in 0..opponents {
            let cursor = missing + opponent * 2;
            hand[0] = pool[cursor];
            hand[1] = pool[cursor + 1];
            opponent_scores.push(evaluate(&hand, variant).score());
        }
        let showdown = resolve_showdown(player_hand.score(), &opponent_scores);
        tally.record(showdown, player_hand.name());
        for index in (0..draw_count).rev() {
            pool.swap(index, swaps[index]);
        }
    }

    tally.finish(Method::MonteCarlo, started)
}

/// Calculates the player's equity and outs for one street.
///
/// Mirrors `calculate(options)` in `js/probability-core.js`:
///
/// * validates the inputs through [`prepare`];
/// * uses [`exact_heads_up`] when there is exactly one opponent and the board
///   already has four or five cards;
/// * uses [`monte_carlo`] otherwise;
/// * always attaches the [`find_improvements`] outs report to the result.
///
/// # Errors
///
/// Returns the [`prepare`] validation errors.
pub fn calculate(
    player: &[Card],
    board: &[Card],
    num_opponents: usize,
    variant: Variant,
    options: &MonteCarloOptions,
) -> Result<EquityResult, ProbError> {
    let prepared = prepare(player, board, num_opponents, variant)?;
    let mut result = if prepared.opponents() == 1 && prepared.board().len() >= 4 {
        exact_heads_up(&prepared)?
    } else {
        monte_carlo(&prepared, options)
    };
    result.outs = Some(find_improvements(player, board, variant));
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use poker_core::Variant;

    fn cards(text: &str) -> Vec<Card> {
        text.split_whitespace()
            .map(|token| token.parse().expect("card"))
            .collect()
    }

    fn options(seed: &str, simulations: u32) -> MonteCarloOptions {
        MonteCarloOptions {
            seed: seed.to_string(),
            simulations,
            min_samples: simulations,
        }
    }

    #[test]
    fn simulation_counts_are_clamped_like_js() {
        assert_eq!(options("seed", 0).target_samples(), 2400);
        assert_eq!(options("seed", 1).target_samples(), 400);
        assert_eq!(options("seed", 10).target_samples(), 400);
        assert_eq!(options("seed", 500).target_samples(), 500);
        assert_eq!(options("seed", 20_000).target_samples(), 12_000);
        assert_eq!(options("seed", 400).effective_min_samples(), 400);
        let default_samples = MonteCarloOptions {
            seed: String::new(),
            simulations: 12_000,
            min_samples: 0,
        };
        assert_eq!(default_samples.effective_min_samples(), 700);
    }

    #[test]
    fn exact_river_splits_tie_equity() {
        let prepared = prepare(
            &cards("2c 3d"),
            &cards("Ts Js Qs Ks As"),
            1,
            Variant::Standard,
        )
        .expect("prepare");
        let result = exact_heads_up(&prepared).expect("exact");
        assert_eq!(result.method, Method::ExactHeadsUp);
        assert_eq!(result.samples, 990);
        assert_eq!(result.win_prob, 0.0);
        assert_eq!(result.tie_prob, 1.0);
        assert_eq!(result.lose_prob, 0.0);
        assert_eq!(result.equity_prob, 0.5);
        assert_eq!(result.best_hand_counts.get("皇家同花顺"), Some(&990));
        assert!(result.outs.is_none());
    }

    #[test]
    fn exact_turn_enumerates_every_river_and_opponent_pair() {
        let prepared =
            prepare(&cards("Ah Kh"), &cards("Qh Jh 2c 7d"), 1, Variant::Standard).expect("prepare");
        let result = exact_heads_up(&prepared).expect("exact");
        assert_eq!(result.method, Method::ExactHeadsUp);
        assert_eq!(result.samples, 45_540);
        assert_eq!(
            result.best_hand_counts.values().sum::<u64>(),
            result.samples
        );
        let total = result.win_prob + result.tie_prob + result.lose_prob;
        assert!((total - 1.0).abs() < 1e-12);
        assert!(result.equity_prob >= result.win_prob);
    }

    #[test]
    fn exact_requires_a_turn_or_river_board() {
        let prepared =
            prepare(&cards("Ah Kh"), &cards("Qh Jh 2c"), 1, Variant::Standard).expect("prepare");
        assert_eq!(
            exact_heads_up(&prepared).unwrap_err(),
            ProbError::ExactRequiresTurnOrRiver { board_cards: 3 }
        );
    }

    #[test]
    fn monte_carlo_is_deterministic_per_seed() {
        let prepared = prepare(&cards("As Ah"), &[], 3, Variant::Standard).expect("prepare");
        let config = options("unit-monte-carlo", 400);
        let first = monte_carlo(&prepared, &config);
        let second = monte_carlo(&prepared, &config);
        assert_eq!(first, second);
        assert_eq!(first.method, Method::MonteCarlo);
        assert_eq!(first.samples, 400);
        assert_eq!(first.best_hand_counts.values().sum::<u64>(), 400);
        assert!(first.outs.is_none());
        assert!(first.win_prob > 0.5);
        let mut other = config.clone();
        other.seed = "unit-monte-carlo-other".to_string();
        assert_ne!(first, monte_carlo(&prepared, &other));
    }

    #[test]
    fn calculate_routes_between_exact_and_monte_carlo() {
        let config = options("unit-route", 400);
        let river = calculate(
            &cards("As Ah"),
            &cards("Ks Qd 9c 7h 2s"),
            1,
            Variant::Standard,
            &config,
        )
        .expect("river");
        assert_eq!(river.method, Method::ExactHeadsUp);
        assert!(river.outs.is_some());

        let flop = calculate(
            &cards("As Ah"),
            &cards("Ks Qd 9c"),
            1,
            Variant::Standard,
            &config,
        )
        .expect("flop");
        assert_eq!(flop.method, Method::MonteCarlo);

        let multiway = calculate(
            &cards("As Ah"),
            &cards("Ks Qd 9c 7h 2s"),
            2,
            Variant::Standard,
            &config,
        )
        .expect("multiway");
        assert_eq!(multiway.method, Method::MonteCarlo);

        let preflop =
            calculate(&cards("As Ah"), &[], 9, Variant::Standard, &config).expect("preflop");
        let outs = preflop.outs.expect("outs");
        assert_eq!(outs.total_outs, 0);
    }

    #[test]
    fn direct_calls_match_calculate_without_outs() {
        let config = options("unit-direct", 400);
        let prepared =
            prepare(&cards("Ah Kh"), &cards("Qh Jh 2c 7d"), 1, Variant::Standard).expect("prepare");
        let mut expected = calculate(
            &cards("Ah Kh"),
            &cards("Qh Jh 2c 7d"),
            1,
            Variant::Standard,
            &config,
        )
        .expect("calculate");
        expected.outs = None;
        assert_eq!(exact_heads_up(&prepared).expect("exact"), expected);

        let prepared =
            prepare(&cards("As Ah"), &cards("Ks Qd 9c"), 2, Variant::Standard).expect("prepare");
        let mut expected = calculate(
            &cards("As Ah"),
            &cards("Ks Qd 9c"),
            2,
            Variant::Standard,
            &config,
        )
        .expect("calculate");
        expected.outs = None;
        assert_eq!(monte_carlo(&prepared, &config), expected);
    }

    #[test]
    fn calculate_reports_prepare_errors() {
        let config = options("unit-errors", 400);
        let error = calculate(&cards("As"), &[], 1, Variant::Standard, &config).unwrap_err();
        assert_eq!(error, ProbError::WrongHoleCardCount { provided: 1 });
        let error = calculate(
            &cards("As Kh"),
            &cards("As 2c 3c"),
            1,
            Variant::Standard,
            &config,
        )
        .unwrap_err();
        assert_eq!(error, ProbError::DuplicateKnownCard);
    }
}
