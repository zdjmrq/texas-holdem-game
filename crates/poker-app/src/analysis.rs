//! Background equity analysis built on poker-prob.
//!
//! A single worker thread computes equity + outs for the human hand; the UI
//! only polls results and never blocks. Results are cached by a stable key of
//! (hole cards, board, opponent count, variant).

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};

use poker_core::{Card, HandCategory, Variant, evaluate};
use poker_prob::{EquityResult, MonteCarloOptions, calculate};

/// Monte Carlo sample budget for the UI. The engine clamps this to 400..=12000.
const ANALYSIS_SIMULATIONS: u32 = 2_400;
const MAX_CACHE_ENTRIES: usize = 48;

/// Everything the analysis panel needs for one decision point.
#[derive(Debug, Clone)]
pub struct AnalysisSummary {
    pub equity: EquityResult,
    pub hand_name: Option<&'static str>,
    pub hand_category: Option<HandCategory>,
    pub hand_rank_index: Option<u8>,
    pub outs_total: u32,
    pub outs_strong: u32,
    pub outs_thin: u32,
}

impl AnalysisSummary {
    pub fn win_pct(&self) -> f64 {
        self.equity.win_prob * 100.0
    }

    pub fn tie_pct(&self) -> f64 {
        self.equity.tie_prob * 100.0
    }

    pub fn lose_pct(&self) -> f64 {
        self.equity.lose_prob * 100.0
    }

    pub fn equity_pct(&self) -> f64 {
        self.equity.equity_prob * 100.0
    }
}

#[derive(Debug, Clone)]
struct Request {
    hole: Vec<Card>,
    board: Vec<Card>,
    opponents: usize,
    variant: Variant,
}

/// Handle to the analysis worker plus a result cache.
pub struct AnalysisService {
    request_tx: Option<Sender<(u64, Request)>>,
    result_rx: Receiver<(u64, Result<AnalysisSummary, String>)>,
    worker: Option<std::thread::JoinHandle<()>>,
    cache: HashMap<u64, Arc<AnalysisSummary>>,
    cache_limit: usize,
    pending: Option<u64>,
    last_error: Option<String>,
}

impl Default for AnalysisService {
    fn default() -> Self {
        Self::new()
    }
}

impl AnalysisService {
    pub fn new() -> Self {
        let (request_tx, request_rx) = channel::<(u64, Request)>();
        let (result_tx, result_rx) = channel::<(u64, Result<AnalysisSummary, String>)>();
        let worker = std::thread::Builder::new()
            .name("poker-analysis".to_owned())
            .spawn(move || {
                while let Ok((key, request)) = request_rx.recv() {
                    let outcome = compute(&request);
                    if result_tx.send((key, outcome)).is_err() {
                        break;
                    }
                }
            })
            .ok();
        Self {
            request_tx: Some(request_tx),
            result_rx,
            worker,
            cache: HashMap::new(),
            cache_limit: MAX_CACHE_ENTRIES,
            pending: None,
            last_error: None,
        }
    }

    /// Request an analysis for a decision point. No-op when the result is
    /// already cached or the exact request is in flight.
    pub fn request(
        &mut self,
        key: u64,
        hole: &[Card],
        board: &[Card],
        opponents: usize,
        variant: Variant,
    ) {
        if self.cache.contains_key(&key) || self.pending == Some(key) {
            return;
        }
        let request = Request {
            hole: hole.to_vec(),
            board: board.to_vec(),
            opponents,
            variant,
        };
        if let Some(tx) = &self.request_tx
            && tx.send((key, request)).is_ok()
        {
            self.pending = Some(key);
        }
    }

    /// Drain finished analyses into the cache.
    pub fn poll(&mut self) {
        loop {
            match self.result_rx.try_recv() {
                Ok((key, outcome)) => {
                    if self.pending == Some(key) {
                        self.pending = None;
                    }
                    match outcome {
                        Ok(summary) => {
                            if self.cache.len() >= self.cache_limit
                                && let Some(first) = self.cache.keys().next().copied()
                            {
                                self.cache.remove(&first);
                            }
                            self.cache.insert(key, Arc::new(summary));
                        }
                        Err(error) => self.last_error = Some(error),
                    }
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => break,
            }
        }
    }

    pub fn get(&self, key: u64) -> Option<Arc<AnalysisSummary>> {
        self.cache.get(&key).cloned()
    }

    pub fn set_cache_limit(&mut self, limit: usize) {
        self.cache_limit = limit.max(1);
        while self.cache.len() > self.cache_limit {
            if let Some(key) = self.cache.keys().next().copied() {
                self.cache.remove(&key);
            }
        }
    }

    pub fn is_pending(&self, key: u64) -> bool {
        self.pending == Some(key)
    }

    pub fn pending_key(&self) -> Option<u64> {
        self.pending
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }
}

impl Drop for AnalysisService {
    fn drop(&mut self) {
        self.request_tx.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Stable cache key for a decision point.
#[must_use]
pub fn analysis_key(hole: &[Card], board: &[Card], opponents: usize, variant: Variant) -> u64 {
    let mut hash: u64 = 1_469_598_103_934_665_603;
    hash = fnv_step(hash, variant as u8);
    hash = fnv_step(hash, opponents as u8);
    hash = fnv_step(hash, hole.len() as u8);
    for card in hole {
        hash = fnv_step(hash, card.index());
    }
    hash = fnv_step(hash, board.len() as u8);
    for card in board {
        hash = fnv_step(hash, card.index());
    }
    hash
}

fn fnv_step(hash: u64, byte: u8) -> u64 {
    (hash ^ u64::from(byte)).wrapping_mul(1_099_511_628_211)
}

fn compute(request: &Request) -> Result<AnalysisSummary, String> {
    let mut cards = request.hole.clone();
    cards.extend_from_slice(&request.board);
    let evaluated = if (2..=7).contains(&cards.len()) {
        Some(evaluate(&cards, request.variant))
    } else {
        None
    };
    let options = MonteCarloOptions {
        seed: "poker-app-analysis".to_owned(),
        simulations: ANALYSIS_SIMULATIONS,
        min_samples: 0,
    };
    let equity = calculate(
        &request.hole,
        &request.board,
        request.opponents,
        request.variant,
        &options,
    )
    .map_err(|error| error.to_string())?;
    let (outs_total, outs_strong, outs_thin) = equity.outs.as_ref().map_or((0, 0, 0), |outs| {
        (outs.total_outs, outs.strong_outs, outs.thin_outs)
    });
    Ok(AnalysisSummary {
        equity,
        hand_name: evaluated.map(|hand| hand.name()),
        hand_category: evaluated.map(|hand| hand.category()),
        hand_rank_index: evaluated.map(|hand| hand.rank_index()),
        outs_total,
        outs_strong,
        outs_thin,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cards(text: &str) -> Vec<Card> {
        text.split_whitespace()
            .map(|token| token.parse().expect("card"))
            .collect()
    }

    #[test]
    fn analysis_key_is_stable_and_sensitive() {
        let hole = cards("As Kh");
        let board = cards("Qs Js 2d");
        let base = analysis_key(&hole, &board, 2, Variant::Standard);
        assert_eq!(base, analysis_key(&hole, &board, 2, Variant::Standard));
        assert_ne!(base, analysis_key(&hole, &board, 3, Variant::Standard));
        assert_ne!(base, analysis_key(&hole, &board, 2, Variant::ShortDeck));
        assert_ne!(
            base,
            analysis_key(&cards("As Qh"), &board, 2, Variant::Standard)
        );
    }

    #[test]
    fn worker_returns_an_analysis_for_a_river_spot() {
        let mut service = AnalysisService::new();
        let hole = cards("Ah Kh");
        let board = cards("Qh Jh Th 2c 7d");
        let key = analysis_key(&hole, &board, 1, Variant::Standard);
        service.request(key, &hole, &board, 1, Variant::Standard);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            service.poll();
            if let Some(summary) = service.get(key) {
                let sum =
                    summary.equity.win_prob + summary.equity.tie_prob + summary.equity.lose_prob;
                assert!((sum - 1.0).abs() < 1e-9, "probabilities must sum to 1");
                assert_eq!(summary.equity.method.as_str(), "exact-heads-up");
                assert_eq!(summary.hand_name, Some("皇家同花顺"));
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "analysis worker did not answer in time"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}
