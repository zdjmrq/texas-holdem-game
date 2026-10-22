//! Behavioural unit tests for the protocol crate (non-golden corners).

use poker_protocol::*;
use serde_json::json;

#[test]
fn client_parse_order_matches_reference_server() {
    // Type check happens before the protocol-version check.
    let error =
        parse_client_message(r#"{"protocolVersion":3,"type":"nope","requestId":"r"}"#).unwrap_err();
    assert!(matches!(error, ClientParseError::ProtocolMismatch { .. }));
    assert_eq!(error.to_server_error().code, ErrorCode::ProtocolMismatch);
    assert_eq!(error.message_text(), "协议版本不匹配（服务端 v2）");

    // Unknown type with an accepted version.
    let error =
        parse_client_message(r#"{"protocolVersion":2,"type":"nope","requestId":"r"}"#).unwrap_err();
    assert_eq!(error.to_server_error().code, ErrorCode::InvalidRequest);
    assert_eq!(error.message_text(), "未知消息类型");

    // Missing type, including a non-string type value.
    let error = parse_client_message(r#"{"protocolVersion":2,"requestId":"r"}"#).unwrap_err();
    assert_eq!(error.message_text(), "缺少消息类型");
    assert_eq!(error.request_id().and_then(RequestId::as_str), Some("r"));
    let error = parse_client_message(r#"{"type":5,"requestId":"r"}"#).unwrap_err();
    assert_eq!(error.message_text(), "缺少消息类型");

    // Invalid JSON.
    let error = parse_client_message("{not json").unwrap_err();
    assert_eq!(error.message_text(), "消息格式无效");
    assert_eq!(error.request_id(), None);
}

#[test]
fn protocol_version_uses_javascript_number_coercion() {
    for accepted in [json!(2), json!("2"), json!(" 2 "), json!([2]), json!(2.0)] {
        let raw = format!(r#"{{"protocolVersion":{accepted},"type":"get_snapshot"}}"#);
        parse_client_message(&raw)
            .unwrap_or_else(|error| panic!("version {accepted} must be accepted: {error}"));
    }
    for rejected in [
        json!(3),
        json!("3"),
        json!(true),
        json!(null),
        json!([]),
        json!([2, 3]),
        json!("2x"),
        json!({}),
    ] {
        let raw = format!(r#"{{"protocolVersion":{rejected},"type":"get_snapshot"}}"#);
        let error = parse_client_message(&raw).unwrap_err();
        assert_eq!(
            error.code(),
            ErrorCode::ProtocolMismatch,
            "version {rejected} must be rejected"
        );
    }
    // Absent protocolVersion is accepted.
    parse_client_message(r#"{"type":"get_snapshot"}"#).unwrap();
}

#[test]
fn request_id_echo_uses_javascript_truthiness() {
    let error = parse_client_message(r#"{"requestId":""}"#).unwrap_err();
    assert_eq!(
        error.request_id(),
        None,
        "empty request id must be echoed as null"
    );
    let reply = serde_json::to_value(error.to_server_error()).unwrap();
    assert_eq!(reply["requestId"], serde_json::Value::Null);

    let error = parse_client_message(r#"{"requestId":"abc"}"#).unwrap_err();
    let reply = serde_json::to_value(error.to_server_error()).unwrap();
    assert_eq!(reply["requestId"], json!("abc"));
}

#[test]
fn known_type_with_bad_shape_is_invalid_shape() {
    let error =
        parse_client_message(r#"{"type":"player_action","requestId":"r","handId":"not-a-number"}"#)
            .unwrap_err();
    assert!(matches!(error, ClientParseError::InvalidShape { .. }));
    assert_eq!(error.to_server_error().code, ErrorCode::InvalidRequest);
    assert_eq!(error.message_text(), "消息格式无效");
    assert_eq!(error.request_id().and_then(RequestId::as_str), Some("r"));
}

#[test]
fn client_messages_announce_protocol_version_on_serialisation() {
    let value = ClientMessage::GetSnapshot(GetSnapshot::default()).to_json_value();
    assert_eq!(value["type"], json!("get_snapshot"));
    assert_eq!(value["protocolVersion"], json!(PROTOCOL_VERSION));
    let parsed = parse_client_message(r#"{"type":"get_snapshot"}"#).unwrap();
    assert_eq!(
        parsed.to_json_value()["protocolVersion"],
        json!(PROTOCOL_VERSION)
    );
}

#[test]
fn server_messages_serialise_envelope_and_nulls() {
    let hello = ServerMessage::Hello(Hello {
        protocol_version: PROTOCOL_VERSION,
        server_id: SERVER_ID.to_owned(),
        server_name: Some(SERVER_NAME.to_owned()),
        app_version: COMPAT_APP_VERSION.to_owned(),
    });
    let value = hello.to_json_value();
    assert_eq!(value["protocolVersion"], json!(PROTOCOL_VERSION));
    assert_eq!(value["type"], json!("hello"));
    assert!(hello.clone().to_json_string().contains("texas-holdem-game"));

    let ack = ServerMessage::Ack(Ack {
        protocol_version: PROTOCOL_VERSION,
        request_id: RequestId::from("req-1"),
        accepted: true,
        room_code: None,
        state_version: 3,
        hand_id: 0,
    });
    let value = ack.to_json_value();
    assert_eq!(value["roomCode"], serde_json::Value::Null);
    assert_eq!(value["requestId"], json!("req-1"));

    let error = ServerMessage::Error(ErrorMessage::new(
        None,
        ErrorCode::ResumeExpired,
        "恢复信息已失效",
    ));
    let value = error.to_json_value();
    assert_eq!(value["requestId"], serde_json::Value::Null);
    assert_eq!(value["code"], json!("RESUME_EXPIRED"));
}

#[test]
fn server_message_parsing_reports_structural_errors() {
    assert_eq!(
        parse_server_message("{"),
        Err(ServerParseError::InvalidJson)
    );
    assert_eq!(
        parse_server_message("[]"),
        Err(ServerParseError::NotAnObject)
    );
    assert_eq!(
        parse_server_message("{}"),
        Err(ServerParseError::MissingType)
    );
    assert!(matches!(
        parse_server_message(r#"{"type":"player_disconnected"}"#),
        Err(ServerParseError::UnknownType(message_type)) if message_type == "player_disconnected"
    ));
    // Unknown message types are ignored by the old client; keep the error
    // distinguishable so a Rust client can choose to skip them.
    assert!(!ServerMessage::is_known_type("player_disconnected"));
    assert!(ServerMessage::is_known_type("game_state"));
}

#[test]
fn error_codes_roundtrip_including_future_ones() {
    for code in [
        ErrorCode::InvalidRequest,
        ErrorCode::ProtocolMismatch,
        ErrorCode::RoomLimit,
        ErrorCode::ResumeExpired,
    ] {
        let value = serde_json::to_value(&code).unwrap();
        assert_eq!(value, json!(code.as_str()));
        let parsed: ErrorCode = serde_json::from_value(value).unwrap();
        assert_eq!(parsed, code);
    }
    let future = ErrorCode::Unknown("SOME_FUTURE_CODE".to_owned());
    let value = serde_json::to_value(&future).unwrap();
    assert_eq!(value, json!("SOME_FUTURE_CODE"));
    assert_eq!(serde_json::from_value::<ErrorCode>(value).unwrap(), future);
}

#[test]
fn bounded_number_matches_javascript_semantics() {
    assert_eq!(bounded_number(None, 450.0, 0.0, 10_000.0, true), 450.0);
    assert_eq!(
        bounded_number(Some(&json!("80")), 450.0, 0.0, 10_000.0, true),
        80.0
    );
    assert_eq!(
        bounded_number(Some(&json!(null)), 450.0, 0.0, 10_000.0, true),
        0.0
    );
    assert_eq!(
        bounded_number(Some(&json!(-5)), 450.0, 0.0, 10_000.0, true),
        0.0
    );
    assert_eq!(
        bounded_number(Some(&json!(1e9)), 450.0, 0.0, 10_000.0, true),
        10_000.0
    );
    assert_eq!(
        bounded_number(Some(&json!(99.9)), 450.0, 0.0, 10_000.0, true),
        99.0
    );
    assert_eq!(
        bounded_number(Some(&json!(true)), 450.0, 0.0, 10_000.0, true),
        1.0
    );
    // Non-finite JSON values are impossible, but the fallback path is used for
    // values JavaScript turns into NaN.
    assert_eq!(
        bounded_number(Some(&json!("abc")), 450.0, 0.0, 10_000.0, true),
        450.0
    );
}

#[test]
fn sanitize_config_uses_blind_fallbacks_and_whitelists() {
    let options: RoomOptionsInput = serde_json::from_value(json!({
        "smallBlind": 40,
        "startingStack": 5000,
        "maxPlayers": 2
    }))
    .unwrap();
    let config = options.sanitize(1_800.0);
    assert_eq!(config.small_blind, 40);
    assert_eq!(config.big_blind, 80);
    assert_eq!(config.ante, 40);
    assert_eq!(config.min_bet, 80);
    assert_eq!(config.starting_stack, 5_000);
    assert_eq!(config.max_players, 2);
    assert_eq!(config.ai_delay, 1_800);
    assert!(!config.is_short_deck);

    let short: RoomOptionsInput = serde_json::from_value(json!({"isShortDeck": "false"})).unwrap();
    assert!(
        short.sanitize(450.0).is_short_deck,
        "non-empty strings are truthy in JS"
    );
    assert_eq!(
        short.sanitize(450.0).max_players,
        6,
        "short deck defaults to six seats"
    );
}

#[test]
fn normalize_name_edge_cases() {
    assert_eq!(normalize_name("  Alice  "), "Alice");
    assert_eq!(
        normalize_name("\u{00a0}Alice\u{00a0}"),
        "Alice",
        "NBSP is JS whitespace"
    );
    assert_eq!(
        normalize_name("a\u{80}b"),
        "a\u{80}b",
        "C1 controls are not stripped by the reference regex"
    );
    assert_eq!(
        normalize_name("\u{feff}Bom"),
        "Bom",
        "BOM is trimmed by JS trim()"
    );
    assert_eq!(normalize_name(""), "玩家");
    assert_eq!(normalize_name("123456789"), "12345678");
    assert_eq!(normalize_name("Ｔｏｍ"), "Tom");
    assert_eq!(normalize_name("ﬁle"), "file");
    assert_eq!(normalize_name("😀😀😀😀😀😀😀😀😀"), "😀😀😀😀😀😀😀😀");
    assert_eq!(normalize_name_value(Some(&json!(42))), "42");
    assert_eq!(normalize_name_value(Some(&json!(0))), "玩家");
    assert_eq!(normalize_name_value(Some(&json!(null))), "玩家");
    assert_eq!(normalize_name_value(Some(&json!([40, 80]))), "40,80");
}

#[test]
fn reconnect_backoff_and_close_codes() {
    assert_eq!(reconnect_delay_ms(0, 0.5), 825);
    assert_eq!(reconnect_delay_ms(4, 0.5), 10_175);
    assert_eq!(reconnect_delay_ms(9, 0.0), RECONNECT_MAX_DELAY_MS);
    assert!(is_terminal_close_code(CLOSE_SESSION_REPLACED));
    assert!(is_terminal_close_code(CLOSE_POLICY_VIOLATION));
    assert!(is_terminal_close_code(CLOSE_MESSAGE_TOO_BIG));
    assert!(is_terminal_close_code(CLOSE_TRY_AGAIN_LATER));
    assert!(is_terminal_close_code(CLOSE_NORMAL));
    assert!(!is_terminal_close_code(1006));
}

#[test]
fn action_types_and_cards_validate_wire_tokens() {
    assert_eq!(ActionType::parse_js("ALLIN"), Some(ActionType::Allin));
    assert_eq!(ActionType::parse_js("Fold"), Some(ActionType::Fold));
    assert_eq!(ActionType::parse_js("raise "), None);
    assert_eq!(ActionType::Allin.as_str(), "allin");
    assert_eq!(Phase::Idle.as_str(), "idle");

    let ace = CardDto::from_value(14, "spades").unwrap();
    assert_eq!(ace, CardDto::new("A", "spades"));
    assert!(ace.is_valid());
    assert_eq!(ace.rank_value(), Some(14));
    assert!(!CardDto::new("1", "spades").is_valid());
    assert!(!CardDto::new("A", "Spades").is_valid());
    assert_eq!(CardDto::from_value(11, "hearts").unwrap().rank, "J");
    assert_eq!(CardDto::from_value(15, "hearts"), None);

    let revealed = RevealedCard::new("7", "clubs", Some(7));
    assert_eq!(
        serde_json::to_value(&revealed).unwrap(),
        json!({"rank":"7","suit":"clubs","value":7})
    );
    assert_eq!(
        serde_json::to_value(RevealedCard::new("7", "clubs", None)).unwrap(),
        json!({"rank":"7","suit":"clubs"})
    );
}

#[test]
fn hello_probe_requires_exact_app_version() {
    let hello = Hello {
        protocol_version: PROTOCOL_VERSION,
        server_id: SERVER_ID.to_owned(),
        server_name: Some(SERVER_NAME.to_owned()),
        app_version: COMPAT_APP_VERSION.to_owned(),
    };
    assert!(hello.is_desktop_probe_compatible(COMPAT_APP_VERSION));
    assert!(!hello.is_desktop_probe_compatible("0.0.0"));
    let newer = Hello {
        app_version: "9.9.9".to_owned(),
        ..hello.clone()
    };
    assert!(!newer.is_desktop_probe_compatible(COMPAT_APP_VERSION));
    let wrong_protocol = Hello {
        protocol_version: 3,
        ..hello
    };
    assert!(!wrong_protocol.is_desktop_probe_compatible(COMPAT_APP_VERSION));
}
