//! Room-configuration whitelist and player-name normalisation.
//!
//! sanitize_config reproduces the reference server's sanitizeConfig() exactly,
//! including JavaScript Number()/truthiness coercion of untrusted JSON fields,
//! because the resulting roomConfig is echoed to every client.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use unicode_normalization::UnicodeNormalization;

use crate::common::RoomConfig;
use crate::constants::{
    AI_DELAY_LEVELS, BLIND_LEVELS, MAX_NAME_LENGTH, MAX_RAW_NAME_LENGTH, STACK_LEVELS,
};
use crate::js;

/// Raw room-option fields as sent in create_room / join_room.
///
/// Values stay JSON so the numeric/boolean coercions of the reference server can
/// be reproduced verbatim; use sanitize() (or sanitize_config()) to obtain the
/// authoritative RoomConfig.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RoomOptionsInput {
    /// Requested seat count.
    #[serde(
        rename = "maxPlayers",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub max_players: Option<Value>,
    /// Variant flag (6+ Hold'em when truthy).
    #[serde(
        rename = "isShortDeck",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub is_short_deck: Option<Value>,
    /// Requested starting stack.
    #[serde(
        rename = "startingStack",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub starting_stack: Option<Value>,
    /// Requested small blind.
    #[serde(
        rename = "smallBlind",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub small_blind: Option<Value>,
    /// Requested big blind.
    #[serde(rename = "bigBlind", default, skip_serializing_if = "Option::is_none")]
    pub big_blind: Option<Value>,
    /// Short-deck ante (fallback for smallBlind).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ante: Option<Value>,
    /// Short-deck minimum bet (fallback for bigBlind).
    #[serde(rename = "minBet", default, skip_serializing_if = "Option::is_none")]
    pub min_bet: Option<Value>,
    /// Requested AI delay.
    #[serde(rename = "aiDelay", default, skip_serializing_if = "Option::is_none")]
    pub ai_delay: Option<Value>,
}

impl RoomOptionsInput {
    /// Sanitises these options with the given server-level AI-delay fallback.
    pub fn sanitize(&self, fallback_ai_delay: f64) -> RoomConfig {
        sanitize_config(self, fallback_ai_delay)
    }
}

fn number_or(value: Option<&Value>, fallback: f64) -> f64 {
    match js::number(value) {
        Some(number) if number != 0.0 && !number.is_nan() => number,
        _ => fallback,
    }
}

fn first_truthy<'value>(
    primary: Option<&'value Value>,
    fallback: Option<&'value Value>,
) -> Option<&'value Value> {
    match primary {
        Some(value) if js::truthy(value) => Some(value),
        _ => fallback,
    }
}

fn bounded_number_value(
    value: f64,
    fallback: f64,
    minimum: f64,
    maximum: f64,
    integer: bool,
) -> f64 {
    let safe = if value.is_finite() { value } else { fallback };
    let bounded = maximum.min(safe).max(minimum);
    if integer { bounded.floor() } else { bounded }
}

/// JavaScript boundedNumber() of the reference server entry point.
///
/// Coerces the JSON value with Number(), substitutes the fallback when the
/// result is not finite, clamps to the inclusive range and optionally floors.
pub fn bounded_number(
    value: Option<&Value>,
    fallback: f64,
    minimum: f64,
    maximum: f64,
    integer: bool,
) -> f64 {
    let parsed = match js::number(value) {
        Some(number) if number.is_finite() => number,
        _ => fallback,
    };
    bounded_number_value(parsed, fallback, minimum, maximum, integer)
}

/// Reproduces the reference server's sanitizeConfig().
///
/// * startingStack must be one of STACK_LEVELS, otherwise 20000;
/// * small/big blind must together match one of BLIND_LEVELS, otherwise (40, 80);
/// * ante/minBet are always mirrored from the selected blind level;
/// * maxPlayers is clamped to 2..=10 (2..=8 for short deck);
/// * aiDelay accepts only AI_DELAY_LEVELS, otherwise it is clamped to the
///   server-level fallback inside 0..=10000.
pub fn sanitize_config(input: &RoomOptionsInput, fallback_ai_delay: f64) -> RoomConfig {
    let is_short_deck = input.is_short_deck.as_ref().is_some_and(js::truthy);

    let starting_candidate = number_or(input.starting_stack.as_ref(), 20_000.0).floor();
    let starting_stack = if STACK_LEVELS
        .iter()
        .any(|level| (*level as f64) == starting_candidate)
    {
        starting_candidate as i64
    } else {
        20_000
    };

    let small_candidate = number_or(
        first_truthy(input.small_blind.as_ref(), input.ante.as_ref()),
        40.0,
    )
    .floor();
    let big_candidate = number_or(
        first_truthy(input.big_blind.as_ref(), input.min_bet.as_ref()),
        80.0,
    )
    .floor();
    let pair = BLIND_LEVELS
        .iter()
        .copied()
        .find(|(small, big)| (*small as f64) == small_candidate && (*big as f64) == big_candidate)
        .unwrap_or((40, 80));

    let max_limit = if is_short_deck { 8.0 } else { 10.0 };
    let default_players = if is_short_deck { 6.0 } else { 10.0 };
    let max_players = number_or(input.max_players.as_ref(), default_players)
        .floor()
        .min(max_limit)
        .max(2.0);

    let requested_ai_delay = js::number(input.ai_delay.as_ref()).map(f64::floor);
    let ai_delay = match requested_ai_delay {
        Some(requested)
            if AI_DELAY_LEVELS
                .iter()
                .any(|level| (*level as f64) == requested) =>
        {
            requested as i64
        }
        _ => bounded_number_value(fallback_ai_delay, 1_800.0, 0.0, 10_000.0, true) as i64,
    };

    RoomConfig {
        max_players: max_players as i64,
        is_short_deck,
        starting_stack,
        small_blind: pair.0,
        big_blind: pair.1,
        ante: pair.0,
        min_bet: pair.1,
        ai_delay,
    }
}

fn slice_utf16(value: &str, limit: usize) -> String {
    let units: Vec<u16> = value.encode_utf16().take(limit).collect();
    // JS slice() counts UTF-16 code units and may split a surrogate pair at the
    // boundary. Such a lone surrogate cannot be represented in a Rust String, so
    // it becomes U+FFFD; this is only reachable with >128 astral characters in
    // the first 256 units and is documented as a known deviation.
    String::from_utf16_lossy(&units)
}

/// Reproduces the reference server's normalizeName() for a string input.
///
/// * truncates the raw input to 256 UTF-16 code units;
/// * applies Unicode NFKC (full-width and compatibility forms collapse);
/// * removes C0 control characters and DEL;
/// * trims JavaScript whitespace;
/// * falls back to "玩家" when empty and truncates to 8 Unicode scalar values.
pub fn normalize_name(value: &str) -> String {
    let truncated = slice_utf16(value, MAX_RAW_NAME_LENGTH);
    let normalized: String = truncated.nfkc().collect();
    let filtered: String = normalized
        .chars()
        .filter(|character| {
            let code = *character as u32;
            !(code <= 0x1f || code == 0x7f)
        })
        .collect();
    let trimmed = filtered.trim_matches(js::is_js_whitespace);
    let base = if trimmed.is_empty() {
        "玩家"
    } else {
        trimmed
    };
    base.chars().take(MAX_NAME_LENGTH).collect()
}

/// Reproduces normalizeName() for an arbitrary JSON value.
///
/// The reference server first evaluates String(value || "玩家"), so falsy JSON
/// values fall back to the default name and truthy scalars/arrays are stringified
/// with JavaScript rules.
pub fn normalize_name_value(value: Option<&Value>) -> String {
    let raw = match value {
        Some(value) if js::truthy(value) => js::string(Some(value)),
        _ => "玩家".to_owned(),
    };
    normalize_name(&raw)
}
