//! Fixed public spot: verify deeper search spends its budget and reduces sampling noise.
use poker_ai::input::RawDecisionContext;
use poker_ai::{AIPlayer, ProfileSource, Style};
use poker_core::Variant;
#[test]
fn deep_search_uses_twenty_thousand_samples_and_reduces_standard_error() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/golden/ai/decisions-standard-general.jsonl"
    ))
    .unwrap();
    let value = text
        .lines()
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
        .find(|v| {
            v["context"]["communityCards"]
                .as_array()
                .is_some_and(|a| a.len() == 3)
        })
        .unwrap();
    let raw: RawDecisionContext = serde_json::from_value({
        let mut c = value["context"].clone();
        c["holeCards"] = value["aiState"]["holeCards"].clone();
        c
    })
    .unwrap();
    let mut base = raw.clone();
    base.mc_samples = Some(400.0);
    let mut deep = raw;
    poker_ai::search::deepen(&mut deep);
    let run = |raw: RawDecisionContext| {
        let ctx = raw.normalize();
        let mut ai = AIPlayer::new(
            "Quality probe",
            Style::Tag,
            ctx.starting_stack,
            ctx.hero_seat,
            Variant::Standard,
            ProfileSource::StandardStyle,
        );
        ai.set_hole_cards(&ctx.hole_cards);
        let start = std::time::Instant::now();
        let d = ai.decide(&ctx);
        assert!(ctx.is_legal(d.action));
        (d.trace.equity, start.elapsed())
    };
    let _ = run(base.clone());
    let (b, bt) = run(base);
    let (d, dt) = run(deep);
    println!(
        "reference: samples={} stderr={} elapsed={:?}; deep: samples={} stderr={} elapsed={:?}",
        b.samples, b.standard_error, bt, d.samples, d.standard_error, dt
    );
    assert_eq!(b.samples, 400);
    assert_eq!(d.samples, 20_000);
    assert!(d.standard_error < b.standard_error * 0.25);
}

#[test]
fn deep_river_search_runs_eight_thousand_iterations_outside_reference_table() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/golden/ai/decisions-synthetic.jsonl"
    ))
    .unwrap();
    let value = text
        .lines()
        .map(|l| serde_json::from_str::<serde_json::Value>(l).unwrap())
        .find(|v| v["expect"]["trace"]["riverSolver"].is_object())
        .unwrap();
    let mut raw: RawDecisionContext = serde_json::from_value({
        let mut c = value["context"].clone();
        c["holeCards"] = value["aiState"]["holeCards"].clone();
        c
    })
    .unwrap();
    raw.table_player_count = Some(2);
    raw.starting_stack = Some(20_000.0);
    poker_ai::search::deepen(&mut raw);
    let ctx = raw.normalize();
    let mut ai = AIPlayer::new(
        "CFR probe",
        Style::Tag,
        20_000.0,
        ctx.hero_seat,
        Variant::Standard,
        ProfileSource::StandardStyle,
    );
    ai.set_hole_cards(&ctx.hole_cards);
    let start = std::time::Instant::now();
    let decision = ai.decide(&ctx);
    println!("deep river elapsed={:?}", start.elapsed());
    assert!(ctx.is_legal(decision.action));
    assert_eq!(decision.trace.river_solver.unwrap().iterations, 8_000);
}
