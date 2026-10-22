//! Audio subsystem for the spike: mp3 playback through rodio 0.23 + symphonia.
//!
//! Two independent concerns are validated:
//!
//! * decoding - works headless, no audio device needed (--audio-test, unit tests);
//! * playback - opens the default OS sink through cpal, keeps the MixerDeviceSink alive for the
//!   lifetime of the app and exposes a pause/play toggle on a Player.
//!
//! The music file is embedded with include_bytes! on purpose: the release app must run from a
//! single portable executable, and embedding removes all "where is assets/?" failure modes.

use std::io::Cursor;
use std::time::Duration;

/// Embedded copy of assets/music/01.mp3 (originally from the reference repo's assets/music).
pub const BACKGROUND_TRACK_MP3: &[u8] = include_bytes!("../../../assets/music/01.mp3");

/// Human readable name of the embedded track.
pub const BACKGROUND_TRACK_NAME: &str = "01.mp3";

/// Facts gathered by decoding the embedded track. No audio device is touched here.
#[derive(Debug, Clone)]
pub struct DecodeProbe {
    pub sample_rate: u32,
    pub channels: u16,
    pub duration_secs: Option<f64>,
    pub decoded_samples: usize,
    /// Peak absolute sample value in the decoded window (0.0 = silence / broken decode).
    pub peak_amplitude: f32,
}

impl DecodeProbe {
    pub fn summary(&self) -> String {
        let duration = self
            .duration_secs
            .map(|secs| format!("{secs:.2}s"))
            .unwrap_or_else(|| "unknown".to_owned());
        format!(
            "{}: {} Hz / {} ch / total {} / decoded {} samples / peak {:.3}",
            BACKGROUND_TRACK_NAME,
            self.sample_rate,
            self.channels,
            duration,
            self.decoded_samples,
            self.peak_amplitude
        )
    }
}

/// Decode up to the given number of seconds of the embedded mp3.
///
/// This exercises the whole decode path (demuxer + mp3 decoder + sample conversion) without
/// requiring an audio device, which makes it usable in tests and on CI.
pub fn decode_probe(seconds: f32) -> Result<DecodeProbe, String> {
    use rodio::Source as _;

    let decoder = rodio::Decoder::try_from(Cursor::new(BACKGROUND_TRACK_MP3.to_vec()))
        .map_err(|error| format!("mp3 decode failed: {error}"))?;
    let sample_rate = decoder.sample_rate().get();
    let channels = decoder.channels().get();
    let duration_secs = decoder
        .total_duration()
        .map(|duration| duration.as_secs_f64());

    let limit = (sample_rate as f32 * channels as f32 * seconds.clamp(0.1, 30.0)) as usize;
    let mut peak_amplitude = 0.0_f32;
    let decoded_samples = decoder
        .take(limit)
        .inspect(|sample| {
            let value: f32 = *sample;
            peak_amplitude = peak_amplitude.max(value.abs());
        })
        .count();

    Ok(DecodeProbe {
        sample_rate,
        channels,
        duration_secs,
        decoded_samples,
        peak_amplitude,
    })
}

/// Handle to the default audio output plus a player for the background track.
pub struct AudioEngine {
    sink: rodio::MixerDeviceSink,
    player: rodio::Player,
}

impl AudioEngine {
    /// Open the default OS sink. Fails gracefully when the machine has no audio device.
    pub fn open() -> Result<Self, String> {
        let sink = rodio::DeviceSinkBuilder::open_default_sink()
            .map_err(|error| format!("open default audio sink failed: {error}"))?;
        let player = rodio::Player::connect_new(sink.mixer());
        player.set_volume(0.35);
        Ok(Self { sink, player })
    }

    /// Human readable device configuration for the status panel and logs.
    pub fn device_summary(&self) -> String {
        let config = self.sink.config();
        format!(
            "{} Hz / {} ch / {:?} / buffer {:?}",
            config.sample_rate().get(),
            config.channel_count().get(),
            config.sample_format(),
            config.buffer_size()
        )
    }

    /// True while the background track is actually advancing.
    pub fn is_active(&self) -> bool {
        !self.player.is_paused() && !self.player.empty()
    }

    pub fn position_secs(&self) -> f64 {
        self.player.get_pos().as_secs_f64()
    }

    /// Play/pause toggle. Appends the embedded track the first time (looped).
    pub fn toggle(&mut self) -> Result<bool, String> {
        if self.is_active() {
            self.player.pause();
            return Ok(false);
        }
        if !self.player.empty() && self.player.is_paused() {
            self.player.play();
            return Ok(true);
        }
        self.append_background_track()?;
        self.player.play();
        Ok(true)
    }

    /// Stop playback and drop everything queued.
    pub fn stop(&mut self) {
        self.player.stop();
    }

    fn append_background_track(&mut self) -> Result<(), String> {
        let decoder = rodio::Decoder::try_from(Cursor::new(BACKGROUND_TRACK_MP3.to_vec()))
            .map_err(|error| format!("mp3 decode failed: {error}"))?;
        // Background music should loop for the whole session.
        self.player.append(decoder.repeat_infinite());
        Ok(())
    }
}

/// Headless probe: decode the embedded mp3 and print device information. Never opens a window.
pub fn print_headless_probe() {
    println!("== poker-app audio probe (headless) ==");
    match decode_probe(3.0) {
        Ok(probe) => println!("decode: OK {}", probe.summary()),
        Err(error) => println!("decode: FAILED {error}"),
    }
    match AudioEngine::open() {
        Ok(engine) => println!("device: OK {}", engine.device_summary()),
        Err(error) => println!("device: UNAVAILABLE {error}"),
    }
}

/// Headless playback probe: open the default sink, play the embedded mp3 and verify that the
/// playback position advances (real evidence that cpal + rodio are driving the device).
pub fn play_headless_probe(seconds: f32) -> Result<(), String> {
    println!("== poker-app audio playback probe ==");
    let probe = decode_probe(2.0)?;
    println!("decode: OK {}", probe.summary());

    let mut engine = AudioEngine::open()?;
    println!("device: OK {}", engine.device_summary());
    let started = engine.toggle()?;
    println!("playback: started={started}");

    let sleep = Duration::from_millis(200);
    let mut waited = 0.0_f32;
    while waited < seconds {
        std::thread::sleep(sleep);
        waited += sleep.as_secs_f32();
    }

    let position = engine.position_secs();
    println!(
        "playback position after {waited:.1}s wall time: {position:.3}s (advancing={})",
        position > 0.1
    );
    engine.stop();
    if position > 0.1 {
        Ok(())
    } else {
        Err(format!(
            "playback position did not advance (position={position:.3}s)"
        ))
    }
}
