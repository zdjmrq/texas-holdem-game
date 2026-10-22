//! Finite tabular CFR compiled once into typed arrays.
//!
//! Port of js/compiled-cfr.js (v1.5.8). The game tree is compiled by one depth
//! first traversal into flat node/edge/information-set arrays; every CFR
//! iteration then runs forward reach and backward regret passes over those
//! arrays with no allocation. The iteration order is part of the behavioural
//! contract (it decides floating point summation order), so the loops below
//! mirror the original JavaScript line by line.

use std::collections::{BTreeMap, HashMap};

use crate::AiError;
use crate::input::RiverPolicyNode;

use super::game::RiverGame;

/// A finite two-player zero-sum game tree that can be compiled for CFR.
///
/// The original JavaScript passes a plain game object with exactly these
/// methods into CompiledCfr; the trait keeps that seam so structural tests can
/// compile small games (for example Kuhn poker) through the same code path.
pub trait GameTree {
    /// Node representation used by the game.
    type State: Clone;

    /// Root states paired with their chance probability, in root order.
    fn roots(&self) -> Vec<(Self::State, f64)>;

    /// True when the node is terminal.
    fn terminal(&self, state: &Self::State) -> bool;

    /// Acting player of a non-terminal node (must be 0 or 1).
    fn player(&self, state: &Self::State) -> u8;

    /// Information-set key for the acting player.
    fn key(&self, state: &Self::State, player: u8) -> String;

    /// Legal actions of a non-terminal node, in menu order.
    fn actions(&self, state: &Self::State) -> Vec<String>;

    /// Child node reached by applying an action.
    fn child(&self, state: &Self::State, action: &str) -> Self::State;

    /// Terminal payoff from player 0's point of view.
    fn payoff(&self, state: &Self::State) -> f64;
}

impl GameTree for RiverGame {
    type State = super::game::State;

    fn roots(&self) -> Vec<(Self::State, f64)> {
        self.initial_states()
    }

    fn terminal(&self, state: &Self::State) -> bool {
        RiverGame::terminal(self, state)
    }

    fn player(&self, state: &Self::State) -> u8 {
        RiverGame::player(self, state)
    }

    fn key(&self, state: &Self::State, player: u8) -> String {
        RiverGame::key(self, state, player)
    }

    fn actions(&self, state: &Self::State) -> Vec<String> {
        RiverGame::actions(self, state)
    }

    fn child(&self, state: &Self::State, action: &str) -> Self::State {
        RiverGame::child(self, state, action)
    }

    fn payoff(&self, state: &Self::State) -> f64 {
        RiverGame::payoff(self, state)
    }
}

/// One information set: key, acting player, action menu and regret slot offset.
#[derive(Clone, Debug, PartialEq)]
pub struct InfoSet {
    /// Information-set key (player:bucket:public-history).
    pub key: String,
    /// Acting player (0 = hero, 1 = villain).
    pub player: u8,
    /// Action menu.
    pub actions: Vec<String>,
    /// Offset of the first action slot in the per-action arrays.
    pub offset: usize,
}

/// A game tree compiled into flat arrays plus CFR working buffers.
#[derive(Clone, Debug, PartialEq)]
pub struct CompiledCfr {
    /// Information sets in first-encounter order (the JS Map insertion order).
    pub infos: Vec<InfoSet>,
    /// Root node indices with their chance probability, in root order.
    pub roots: Vec<(usize, f64)>,
    /// Information-set index per node (-1 = terminal).
    pub info_index: Vec<i32>,
    /// First child edge index per node.
    pub child_offset: Vec<i32>,
    /// Number of children per node.
    pub child_count: Vec<i32>,
    /// Child node indices.
    pub children: Vec<i32>,
    /// Terminal payoff per node (overwritten with node values during iteration).
    pub values: Vec<f64>,
    /// Player 0 reach probability per node.
    pub r0: Vec<f64>,
    /// Player 1 reach probability per node.
    pub r1: Vec<f64>,
    /// Chance (deal) probability per node.
    pub chance: Vec<f64>,
    /// Cumulative counterfactual regrets per information-set action slot.
    pub regrets: Vec<f64>,
    /// Cumulative average strategy per information-set action slot.
    pub average: Vec<f64>,
    /// Current regret-matched strategy per information-set action slot.
    pub sigma: Vec<f64>,
    /// Regret delta of the current iteration per information-set action slot.
    pub delta: Vec<f64>,
    /// Completed CFR iterations.
    pub iterations: u32,
}

/// One node while the tree is being compiled.
struct Record {
    info: i32,
    children: Vec<i32>,
    payoff: f64,
}

/// Depth-first compiler shared by all game trees.
fn visit<G: GameTree>(
    game: &G,
    state: &G::State,
    records: &mut Vec<Record>,
    infos: &mut Vec<InfoSet>,
    lookup: &mut HashMap<String, usize>,
    slots: &mut usize,
) -> Result<i32, AiError> {
    let index = i32::try_from(records.len())
        .map_err(|_| AiError::RiverInvalid("game tree is too large"))?;
    records.push(Record {
        info: -1,
        children: Vec::new(),
        payoff: 0.0,
    });
    if game.terminal(state) {
        records[usize::try_from(index).expect("non-negative node index")].payoff =
            game.payoff(state);
        return Ok(index);
    }
    let player = game.player(state);
    if player > 1 {
        return Err(AiError::RiverInvalid(
            "two players, no internal chance nodes",
        ));
    }
    let key = game.key(state, player);
    let actions = game.actions(state);
    let info = match lookup.get(&key) {
        Some(&existing) => existing,
        None => {
            let new_index = infos.len();
            let offset = *slots;
            *slots += actions.len();
            infos.push(InfoSet {
                key: key.clone(),
                player,
                actions: actions.clone(),
                offset,
            });
            lookup.insert(key, new_index);
            new_index
        }
    };
    let existing = &infos[info];
    if existing.player != player || existing.actions != actions {
        return Err(AiError::RiverInvalid("inconsistent information set"));
    }
    let mut children = Vec::with_capacity(actions.len());
    for action in &actions {
        let child_state = game.child(state, action);
        children.push(visit(game, &child_state, records, infos, lookup, slots)?);
    }
    let record = &mut records[usize::try_from(index).expect("non-negative node index")];
    record.info =
        i32::try_from(info).map_err(|_| AiError::RiverInvalid("game tree is too large"))?;
    record.children = children;
    Ok(index)
}

impl CompiledCfr {
    /// Compiles a finite game tree. Chance is fully expanded by the roots.
    ///
    /// # Errors
    ///
    /// Returns AiError::RiverInvalid for invalid deal probabilities, internal
    /// chance nodes, inconsistent information sets or oversized trees.
    pub fn compile<G: GameTree>(game: &G) -> Result<Self, AiError> {
        let mut infos: Vec<InfoSet> = Vec::new();
        let mut lookup: HashMap<String, usize> = HashMap::new();
        let mut records: Vec<Record> = Vec::new();
        let mut slots = 0usize;
        let mut roots: Vec<(usize, f64)> = Vec::new();
        for (state, probability) in game.roots() {
            if !probability.is_finite() || probability < 0.0 {
                return Err(AiError::RiverInvalid("invalid deal probability"));
            }
            let root = visit(
                game,
                &state,
                &mut records,
                &mut infos,
                &mut lookup,
                &mut slots,
            )?;
            roots.push((
                usize::try_from(root).expect("non-negative node index"),
                probability,
            ));
        }
        let count = records.len();
        let info_index: Vec<i32> = records.iter().map(|record| record.info).collect();
        let mut child_offset = vec![0i32; count];
        let mut child_count = vec![0i32; count];
        let mut children: Vec<i32> = Vec::new();
        for (index, record) in records.iter().enumerate() {
            child_offset[index] = i32::try_from(children.len())
                .map_err(|_| AiError::RiverInvalid("game tree is too large"))?;
            child_count[index] = i32::try_from(record.children.len())
                .map_err(|_| AiError::RiverInvalid("game tree is too large"))?;
            children.extend_from_slice(&record.children);
        }
        let values: Vec<f64> = records.iter().map(|record| record.payoff).collect();
        let mut r0 = vec![0.0f64; count];
        let mut r1 = vec![0.0f64; count];
        let mut chance = vec![0.0f64; count];
        for &(node, probability) in &roots {
            chance[node] = probability;
            r0[node] = 1.0;
            r1[node] = 1.0;
        }
        for node in 0..count {
            let offset = usize::try_from(child_offset[node]).expect("non-negative offset");
            let count_children = usize::try_from(child_count[node]).expect("non-negative count");
            for edge in 0..count_children {
                let child = usize::try_from(children[offset + edge]).expect("non-negative child");
                chance[child] = chance[node];
            }
        }
        Ok(Self {
            infos,
            roots,
            info_index,
            child_offset,
            child_count,
            children,
            values,
            r0,
            r1,
            chance,
            regrets: vec![0.0f64; slots],
            average: vec![0.0f64; slots],
            sigma: vec![0.0f64; slots],
            delta: vec![0.0f64; slots],
            iterations: 0,
        })
    }

    /// Number of compiled tree nodes.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.info_index.len()
    }

    /// One CFR iteration: regret matching, forward reach, backward values.
    pub fn iteration(&mut self) {
        let Self {
            infos,
            info_index,
            child_offset,
            child_count,
            children,
            values,
            r0,
            r1,
            chance,
            regrets,
            average,
            sigma,
            delta,
            iterations,
            ..
        } = self;
        // 1. Regret matching.
        for info in infos.iter() {
            let action_count = info.actions.len();
            let regret_slice = &regrets[info.offset..info.offset + action_count];
            let sum: f64 = regret_slice.iter().map(|regret| regret.max(0.0)).sum();
            let sigma_slice = &mut sigma[info.offset..info.offset + action_count];
            for (probability, regret) in sigma_slice.iter_mut().zip(regret_slice.iter()) {
                *probability = if sum != 0.0 {
                    regret.max(0.0) / sum
                } else {
                    1.0 / action_count as f64
                };
            }
        }
        // 2. Forward reach probabilities and average-strategy accumulation.
        for &(root, _) in &self.roots {
            r0[root] = 1.0;
            r1[root] = 1.0;
        }
        delta.fill(0.0);
        for node in 0..info_index.len() {
            let info_index_value = info_index[node];
            if info_index_value < 0 {
                continue;
            }
            let info = &infos[usize::try_from(info_index_value).expect("non-negative info index")];
            let offset = info.offset;
            let edge_offset = usize::try_from(child_offset[node]).expect("non-negative offset");
            let count = usize::try_from(child_count[node]).expect("non-negative count");
            for action in 0..count {
                let child = usize::try_from(children[edge_offset + action])
                    .expect("non-negative child index");
                let probability = sigma[offset + action];
                r0[child] = r0[node] * if info.player == 0 { probability } else { 1.0 };
                r1[child] = r1[node] * if info.player == 1 { probability } else { 1.0 };
                let reach = if info.player == 0 { r0[node] } else { r1[node] };
                average[offset + action] += (chance[node] * reach) * probability;
            }
        }
        // 3. Backward node values and regret deltas.
        for node in (0..info_index.len()).rev() {
            let info_index_value = info_index[node];
            if info_index_value < 0 {
                continue;
            }
            let info = &infos[usize::try_from(info_index_value).expect("non-negative info index")];
            let offset = info.offset;
            let edge_offset = usize::try_from(child_offset[node]).expect("non-negative offset");
            let count = usize::try_from(child_count[node]).expect("non-negative count");
            let mut value = 0.0f64;
            for action in 0..count {
                let child = usize::try_from(children[edge_offset + action])
                    .expect("non-negative child index");
                value += sigma[offset + action] * values[child];
            }
            values[node] = value;
            let weight = chance[node]
                * if info.player == 0 {
                    r1[node]
                } else {
                    -r0[node]
                };
            for action in 0..count {
                let child = usize::try_from(children[edge_offset + action])
                    .expect("non-negative child index");
                delta[offset + action] += weight * (values[child] - value);
            }
        }
        // 4. Accumulate regrets.
        for (regret, delta_value) in regrets.iter_mut().zip(delta.iter()) {
            *regret += *delta_value;
        }
        *iterations += 1;
    }

    /// Runs the requested number of CFR iterations.
    ///
    /// # Errors
    ///
    /// Returns AiError::RiverInvalid unless the count is in 1..=100000, matching
    /// the original CompiledCfr.run validation.
    pub fn run(&mut self, iterations: u32) -> Result<(), AiError> {
        if iterations == 0 || iterations > 100_000 {
            return Err(AiError::RiverInvalid("finite iteration count required"));
        }
        for _ in 0..iterations {
            self.iteration();
        }
        Ok(())
    }

    /// Average-strategy policy, one node per information set.
    #[must_use]
    pub fn policy(&self) -> BTreeMap<String, RiverPolicyNode> {
        let mut policy = BTreeMap::new();
        for info in &self.infos {
            let mut sum = 0.0f64;
            for action in 0..info.actions.len() {
                sum += self.average[info.offset + action];
            }
            let probabilities = info
                .actions
                .iter()
                .enumerate()
                .map(|(action, _)| {
                    if sum != 0.0 {
                        self.average[info.offset + action] / sum
                    } else {
                        1.0 / info.actions.len() as f64
                    }
                })
                .collect::<Vec<f64>>();
            policy.insert(
                info.key.clone(),
                RiverPolicyNode {
                    player: info.player,
                    actions: info.actions.clone(),
                    probabilities,
                },
            );
        }
        policy
    }
}
