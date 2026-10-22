//! Frozen client-behaviour constants and the desktop hello probe, checked
//! against tests/golden/protocol/ (captured from the reference implementation).

mod support;

use std::time::Duration;

use poker_client::{RequestKind, probe_server_with_timeout};
use poker_protocol::constants::{
    RECONNECT_BASE_DELAY_MS, RECONNECT_JITTER_MS, RECONNECT_MAX_DELAY_MS, RECONNECT_MAX_DOUBLINGS,
    SESSION_STORAGE_KEY, is_terminal_close_code, reconnect_delay_ms,
};
use support::{golden_json, golden_lines, read_value, send_value, serve};

#[test]
fn terminal_close_codes_match_client_behaviour() {
    let golden = golden_json("client-behaviour.json");
    let codes: Vec<u16> = golden["terminalCloseCodes"]
        .as_array()
        .expect("terminalCloseCodes")
        .iter()
        .map(|code| u16::try_from(code.as_u64().expect("code")).expect("u16"))
        .collect();
    for code in &codes {
        assert!(is_terminal_close_code(*code), "{code} must be terminal");
    }
    for code in [1005u16, 1006, 1011, 1012, 1014, 3000, 4000, 4002] {
        assert!(
            !is_terminal_close_code(code),
            "{code} must trigger an automatic reconnect"
        );
    }
    // The 1000 code is terminal for the client (it only closes voluntarily).
    assert!(codes.contains(&1000));
}

#[test]
fn reconnect_backoff_constants_match_client_behaviour() {
    let golden = golden_json("client-behaviour.json");
    assert_eq!(
        golden["reconnectBaseDelayMs"].as_u64(),
        Some(RECONNECT_BASE_DELAY_MS)
    );
    assert_eq!(
        golden["reconnectDelayCapMs"].as_u64(),
        Some(RECONNECT_MAX_DELAY_MS)
    );
    assert_eq!(
        golden["reconnectJitterMs"].as_u64(),
        Some(RECONNECT_JITTER_MS)
    );
    assert_eq!(
        golden["reconnectMaxDoublings"].as_u64(),
        Some(u64::from(RECONNECT_MAX_DOUBLINGS))
    );
}

#[test]
fn reconnect_delays_match_client_behaviour() {
    let golden = golden_json("client-behaviour.json");
    for entry in golden["reconnectDelaysMs"].as_array().expect("delays") {
        let attempt = u32::try_from(entry["attempt"].as_u64().expect("attempt")).expect("u32");
        let expected = entry["delayMs"].as_u64().expect("delayMs");
        // The golden corpus was generated with a jitter fraction of 0.5
        // (floor(0.5 * 350) = 175 ms added to every base delay).
        assert_eq!(
            reconnect_delay_ms(attempt, 0.5),
            expected,
            "attempt {attempt}"
        );
    }
    // Boundaries: no jitter, maximum jitter, and the doubling cap.
    assert_eq!(reconnect_delay_ms(0, 0.0), 650);
    assert_eq!(reconnect_delay_ms(0, 0.9999), 999);
    assert_eq!(reconnect_delay_ms(0, 1.0), 1000);
    assert_eq!(reconnect_delay_ms(4, 0.0), 10_000);
    assert_eq!(reconnect_delay_ms(5, 0.0), 10_000);
    assert_eq!(reconnect_delay_ms(9, 0.0), 10_000);
}

#[test]
fn request_id_prefixes_match_client_behaviour() {
    let golden = golden_json("client-behaviour.json");
    let golden_prefixes: Vec<String> = golden["requestIdPrefixes"]
        .as_array()
        .expect("requestIdPrefixes")
        .iter()
        .map(|value| value.as_str().expect("prefix").to_owned())
        .collect();
    let crate_prefixes: Vec<String> = RequestKind::ALL
        .iter()
        .map(|kind| kind.prefix().to_owned())
        .collect();
    assert_eq!(crate_prefixes, golden_prefixes);
    assert_eq!(
        golden["sessionStorageKey"].as_str(),
        Some(SESSION_STORAGE_KEY)
    );
}

#[tokio::test]
async fn desktop_probe_matches_hello_probe_corpus() {
    for case in golden_lines("hello-probe-cases.jsonl") {
        let hello = case["hello"].clone();
        let expected = case["expected"].as_bool().expect("expected");
        let payload = hello.clone();
        let server = serve(move |_index, mut ws, _sink| {
            let payload = payload.clone();
            async move {
                send_value(&mut ws, &payload).await;
                while read_value(&mut ws).await.is_some() {}
            }
        })
        .await;
        let outcome = probe_server_with_timeout(&server.url, Duration::from_secs(2), "1.5.8").await;
        assert_eq!(
            outcome.is_ok(),
            expected,
            "hello probe case {hello}: {outcome:?}"
        );
    }
}

#[tokio::test]
async fn probe_reports_the_reference_hello_fields() {
    let server = serve(|_index, mut ws, _sink| async move {
        support::send_hello(&mut ws).await;
        while read_value(&mut ws).await.is_some() {}
    })
    .await;
    let hello = probe_server_with_timeout(&server.url, Duration::from_secs(2), "1.5.8")
        .await
        .expect("probe");
    assert_eq!(hello.server_id, "texas-holdem-game");
    assert_eq!(hello.protocol_version, 2);
    assert_eq!(hello.app_version, "1.5.8");
}
