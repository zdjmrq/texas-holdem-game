//! Room and seat data structures (no I/O, no async).
//!
//! The room layer mirrors the authoritative poker-engine seat state after every
//! game mutation, so lobby messages and game snapshots share one representation.
//! All mutation functions that also need to broadcast live in server.rs; this
//! module only owns the data and the small pure helpers (seat lookup, host
//! election, timer tokens, dedupe cache).

use std::collections::{BTreeSet, HashMap, VecDeque};

use poker_core::{Card, RuntimeRng};
use poker_engine::{Game, GameConfig, LastAction, Player};
use poker_protocol as proto;

use crate::ai::{AI_AVATARS, AI_NAMES, AiFactory, ServerAi};

/// Avatar of a human seat (the reference server uses one emoji).
pub(crate) const HUMAN_AVATAR: &str = "👤";

/// Capacity of the per-player player_action dedupe cache (reference: 64).
pub(crate) const DEDUPE_CAPACITY: usize = 64;

/// FIFO cache of the most recent action_ack replies per player.
///
/// Duplicate request ids replay the cached reply instead of executing the
/// action again, matching the reference server's reliability contract.
#[derive(Debug, Default)]
pub(crate) struct DedupeCache {
    entries: HashMap<String, String>,
    order: VecDeque<String>,
}

impl DedupeCache {
    /// Looks up a cached reply by its structured request-id key.
    pub(crate) fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(String::as_str)
    }

    /// Stores a reply, evicting the oldest entry when the capacity is exceeded.
    pub(crate) fn insert(&mut self, key: String, reply: String) {
        if self.entries.insert(key.clone(), reply).is_none() {
            self.order.push_back(key);
        }
        while self.entries.len() > DEDUPE_CAPACITY {
            if let Some(oldest) = self.order.pop_front() {
                self.entries.remove(&oldest);
            }
        }
    }
}

/// Turn a request id into a stable cache key.
///
/// The reference server keys the cache by the raw request id value; the JSON
/// encoding keeps string and number ids distinct while staying deterministic.
pub(crate) fn request_key(request_id: &proto::RequestId) -> String {
    serde_json::to_string(request_id.as_value()).unwrap_or_else(|_| "null".to_owned())
}

/// One seat at the table.
///
/// Game-time fields (stack, cards, bets, action) mirror the engine player after
/// every mutation; room-only fields (connection, token, AI brain, dedupe cache)
/// never enter the engine.
pub(crate) struct Seat {
    pub(crate) id: String,
    pub(crate) seat_id: usize,
    pub(crate) name: String,
    pub(crate) avatar: String,
    pub(crate) is_human: bool,
    pub(crate) connected: bool,
    pub(crate) left: bool,
    pub(crate) stack: u64,
    pub(crate) hole_cards: Vec<Card>,
    pub(crate) chips_in_pot: u64,
    pub(crate) round_bet: u64,
    pub(crate) folded: bool,
    pub(crate) is_all_in: bool,
    pub(crate) dealt_in: bool,
    pub(crate) last_action: Option<LastAction>,
    pub(crate) resume_token: String,
    pub(crate) joined_at: u64,
    pub(crate) ai: Option<Box<dyn ServerAi>>,
    pub(crate) processed: DedupeCache,
    pub(crate) conn_id: Option<u64>,
    pub(crate) disconnect_token: Option<u64>,
}

impl Seat {
    /// Builds a human seat.
    pub(crate) fn human(
        id: String,
        seat_id: usize,
        name: String,
        starting_stack: u64,
        resume_token: String,
        joined_at: u64,
        conn_id: u64,
    ) -> Self {
        Self {
            id,
            seat_id,
            name,
            avatar: HUMAN_AVATAR.to_owned(),
            is_human: true,
            connected: true,
            left: false,
            stack: starting_stack,
            hole_cards: Vec::new(),
            chips_in_pot: 0,
            round_bet: 0,
            folded: false,
            is_all_in: false,
            dealt_in: false,
            last_action: None,
            resume_token,
            joined_at,
            ai: None,
            processed: DedupeCache::default(),
            conn_id: Some(conn_id),
            disconnect_token: None,
        }
    }

    /// Builds an AI seat with a brain from the factory.
    pub(crate) fn ai(seat_id: usize, starting_stack: u64, factory: &dyn AiFactory) -> Self {
        let slot = seat_id % AI_NAMES.len();
        Self {
            id: format!("ai-{seat_id}"),
            seat_id,
            name: AI_NAMES[slot].to_owned(),
            avatar: AI_AVATARS[slot].to_owned(),
            is_human: false,
            connected: true,
            left: false,
            stack: starting_stack,
            hole_cards: Vec::new(),
            chips_in_pot: 0,
            round_bet: 0,
            folded: false,
            is_all_in: false,
            dealt_in: false,
            last_action: None,
            resume_token: String::new(),
            joined_at: 0,
            ai: Some(factory.create(seat_id, starting_stack)),
            processed: DedupeCache::default(),
            conn_id: None,
            disconnect_token: None,
        }
    }

    /// True when the seat can be dealt into the next hand
    /// (mirrors poker-engine Player::active_for_new_hand).
    pub(crate) fn active_for_new_hand(&self) -> bool {
        self.stack > 0 && !self.left && (!self.is_human || self.connected)
    }

    /// Converts this seat to an engine player for a fresh table.
    pub(crate) fn to_engine_player(&self) -> Player {
        let mut player = Player::new(self.id.clone(), self.seat_id, self.name.clone(), self.stack);
        player.is_human = self.is_human;
        player.connected = self.connected;
        player.left = self.left;
        player
    }

    /// Copies the authoritative engine player state back into this seat.
    pub(crate) fn sync_from_engine(&mut self, player: &Player) {
        self.stack = player.stack;
        self.hole_cards = player.hole_cards.clone();
        self.chips_in_pot = player.chips_in_pot;
        self.round_bet = player.round_bet;
        self.folded = player.folded;
        self.is_all_in = player.is_all_in;
        self.dealt_in = player.dealt_in;
        self.last_action = player.last_action;
    }
}

/// One room: configuration, seats, optional game and timer generation tokens.
pub(crate) struct Room {
    pub(crate) code: String,
    pub(crate) config: proto::RoomConfig,
    pub(crate) engine_config: GameConfig,
    pub(crate) seats: Vec<Seat>,
    pub(crate) host_id: Option<String>,
    pub(crate) game: Option<Game>,
    pub(crate) state_version: i64,
    pub(crate) ready: BTreeSet<String>,
    pub(crate) rng: RuntimeRng,
    next_token: u64,
    pub(crate) ai_token: Option<u64>,
    pub(crate) next_hand_token: Option<u64>,
    pub(crate) cleanup_token: Option<u64>,
}

impl Room {
    /// Creates a room from a sanitised protocol config.
    pub(crate) fn new(code: String, config: proto::RoomConfig, rng: RuntimeRng) -> Self {
        let engine_config = engine_config(&config);
        Self {
            code,
            config,
            engine_config,
            seats: Vec::new(),
            host_id: None,
            game: None,
            state_version: 0,
            ready: BTreeSet::new(),
            rng,
            next_token: 0,
            ai_token: None,
            next_hand_token: None,
            cleanup_token: None,
        }
    }

    /// Allocates a monotonically increasing generation token for a timer.
    pub(crate) fn alloc_token(&mut self) -> u64 {
        self.next_token += 1;
        self.next_token
    }

    /// Seat index by player id.
    pub(crate) fn seat_index(&self, id: &str) -> Option<usize> {
        self.seats.iter().position(|seat| seat.id == id)
    }

    /// Smallest free seat index (capacity is roomConfig.maxPlayers).
    pub(crate) fn free_seat_id(&self) -> Option<usize> {
        let capacity = usize::try_from(self.config.max_players).unwrap_or(10);
        let occupied: BTreeSet<usize> = self.seats.iter().map(|seat| seat.seat_id).collect();
        (0..capacity).find(|seat| !occupied.contains(seat))
    }

    /// Inserts a seat keeping the list sorted by seat id.
    pub(crate) fn insert_seat(&mut self, seat: Seat) {
        let position = self
            .seats
            .iter()
            .position(|existing| existing.seat_id > seat.seat_id)
            .unwrap_or(self.seats.len());
        self.seats.insert(position, seat);
    }

    /// Number of seats that can be dealt into the next hand.
    pub(crate) fn active_for_new_hand(&self) -> usize {
        self.seats
            .iter()
            .filter(|seat| seat.active_for_new_hand())
            .count()
    }

    /// Elects the host: earliest-joined connected human that has not left.
    pub(crate) fn next_host(&self) -> Option<String> {
        self.seats
            .iter()
            .filter(|seat| seat.is_human && seat.connected && !seat.left)
            .min_by_key(|seat| seat.joined_at)
            .map(|seat| seat.id.clone())
    }

    /// Copies the game player state back into the room seats.
    pub(crate) fn sync_from_game(&mut self) {
        let Some(game) = self.game.as_ref() else {
            return;
        };
        for seat in &mut self.seats {
            if let Some(player) = game.players().iter().find(|player| player.id == seat.id) {
                seat.sync_from_engine(player);
            }
        }
    }

    /// Hand id of the current/last game, or 0 before the first deal.
    pub(crate) fn hand_id(&self) -> i64 {
        self.game
            .as_ref()
            .map_or(0, |game| i64::try_from(game.hand_id()).unwrap_or(0))
    }

    /// Turn id of the current/last game, or 0 before the first deal.
    pub(crate) fn turn_id(&self) -> i64 {
        self.game
            .as_ref()
            .map_or(0, |game| i64::try_from(game.turn_id()).unwrap_or(0))
    }

    /// True when the table is idle after a settled hand.
    pub(crate) fn idle_after_hand(&self) -> bool {
        match self.game.as_ref() {
            Some(game) => {
                game.phase() == poker_engine::Phase::Idle && game.last_hand_result().is_some()
            }
            None => false,
        }
    }
}

/// Builds the engine config from the sanitised protocol room config.
pub(crate) fn engine_config(config: &proto::RoomConfig) -> GameConfig {
    let variant = if config.is_short_deck {
        poker_core::Variant::ShortDeck
    } else {
        poker_core::Variant::Standard
    };
    GameConfig {
        variant,
        starting_stack: u64::try_from(config.starting_stack).unwrap_or(0),
        small_blind: u64::try_from(config.small_blind).unwrap_or(0),
        big_blind: u64::try_from(config.big_blind).unwrap_or(0),
        ante: u64::try_from(config.ante).unwrap_or(0),
        min_bet: u64::try_from(config.min_bet).unwrap_or(0),
        max_players: usize::try_from(config.max_players).unwrap_or(10),
    }
}
