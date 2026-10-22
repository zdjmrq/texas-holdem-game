//! Headless audio checks: the embedded mp3 must decode without any audio device.
//!
//! These tests are the CI-safe half of the audio spike; actual playback is verified separately
//! with the --audio-play command line probe on a machine that has a sound device.

#[test]
fn embedded_mp3_decodes_without_audio_device() {
    let probe = poker_app::audio::decode_probe(2.0).expect("embedded mp3 should decode");
    assert!(
        probe.sample_rate >= 8_000,
        "unexpected sample rate: {}",
        probe.sample_rate
    );
    assert!(
        probe.channels >= 1,
        "unexpected channel count: {}",
        probe.channels
    );
    assert!(
        probe.decoded_samples > 0,
        "decoder produced no samples (peak {})",
        probe.peak_amplitude
    );
    assert!(
        probe.peak_amplitude > 0.001,
        "decoded window is silent, decode may have failed: peak {}",
        probe.peak_amplitude
    );
    if let Some(duration) = probe.duration_secs {
        assert!(
            duration > 10.0,
            "unexpected embedded track duration: {duration:.2}s"
        );
    }
}

#[test]
fn decode_probe_is_deterministic() {
    let first = poker_app::audio::decode_probe(1.0).expect("decode embedded mp3 once");
    let second = poker_app::audio::decode_probe(1.0).expect("decode embedded mp3 twice");
    assert_eq!(first.decoded_samples, second.decoded_samples);
    assert_eq!(first.sample_rate, second.sample_rate);
    assert_eq!(first.channels, second.channels);
}
