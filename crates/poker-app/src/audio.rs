//! Audio subsystem: background music playlist, prompt tones and headless probes.
//!
//! Music files are loaded from assets/music at runtime (the portable Windows zip
//! ships them next to the executable). The optional embedded-music feature embeds
//! the first track as a fallback when no assets directory is available. Prompt
//! tones are synthesised, so no extra assets are required.

use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::time::Duration;

use rodio::Source as _;

use crate::assets;
use crate::settings::PlayMode;

/// Headless decode facts; no audio device is touched.
#[derive(Debug, Clone)]
pub struct DecodeProbe {
    pub sample_rate: u32,
    pub channels: u16,
    pub duration_secs: Option<f64>,
    pub decoded_samples: usize,
    pub peak_amplitude: f32,
    pub source: String,
}

impl DecodeProbe {
    pub fn summary(&self) -> String {
        let duration = self
            .duration_secs
            .map(|secs| format!("{secs:.2}s"))
            .unwrap_or_else(|| "unknown".to_owned());
        format!(
            "{}: {} Hz / {} ch / total {} / decoded {} samples / peak {:.3}",
            self.source,
            self.sample_rate,
            self.channels,
            duration,
            self.decoded_samples,
            self.peak_amplitude
        )
    }
}

/// Bytes used by the headless decode probe.
pub fn probe_bytes() -> Result<(String, Vec<u8>), String> {
    if let Some(path) = assets::music_tracks().into_iter().next() {
        let name = track_title(&path);
        let bytes = std::fs::read(&path)
            .map_err(|error| format!("read music {}: {error}", path.display()))?;
        return Ok((name, bytes));
    }
    #[cfg(feature = "embedded-music")]
    {
        return Ok((
            "embedded 01.mp3".to_owned(),
            assets::EMBEDDED_MUSIC_01.to_vec(),
        ));
    }
    #[cfg(not(feature = "embedded-music"))]
    {
        Err("no music assets found under assets/music".to_owned())
    }
}

/// Decode up to the given number of seconds through rodio's symphonia decoder.
pub fn decode_probe(seconds: f32) -> Result<DecodeProbe, String> {
    let (source, bytes) = probe_bytes()?;
    decode_bytes(&source, bytes, seconds)
}

fn decode_bytes(source: &str, bytes: Vec<u8>, seconds: f32) -> Result<DecodeProbe, String> {
    let decoder = rodio::Decoder::try_from(std::io::Cursor::new(bytes))
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
        source: source.to_owned(),
    })
}

fn track_title(path: &std::path::Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .map_or_else(|| path.display().to_string(), ToOwned::to_owned)
}

/// Next track index for a playlist mode (pure helper, unit tested).
#[must_use]
pub fn next_track_index(
    mode: PlayMode,
    current: usize,
    len: usize,
    shuffle_state: u64,
) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let current = current.min(len - 1);
    match mode {
        PlayMode::RepeatOne => Some(current),
        PlayMode::Sequential => Some((current + 1) % len),
        PlayMode::Shuffle => {
            if len == 1 {
                return Some(0);
            }
            let mut next = (shuffle_state % len as u64) as usize;
            if next == current {
                next = (next + 1) % len;
            }
            Some(next)
        }
    }
}

/// Background music + prompt-tone manager. The device is opened lazily so a
/// missing device or disabled music never blocks application startup.
pub struct AudioManager {
    music_player: Option<rodio::Player>,
    prompt_player: Option<rodio::Player>,
    sink: Option<rodio::MixerDeviceSink>,
    tracks: Vec<PathBuf>,
    current: Option<usize>,
    enabled: bool,
    sound_enabled: bool,
    mode: PlayMode,
    volume: f32,
    shuffle_state: u64,
    last_error: Option<String>,
}

impl AudioManager {
    pub fn new(no_audio: bool) -> Self {
        let tracks = if no_audio {
            Vec::new()
        } else {
            assets::music_tracks()
        };
        let current = if tracks.is_empty() { None } else { Some(0) };
        Self {
            music_player: None,
            prompt_player: None,
            sink: None,
            tracks,
            current,
            enabled: false,
            sound_enabled: true,
            mode: PlayMode::Sequential,
            volume: 0.35,
            shuffle_state: 0x9e37_79b9_7f4a_7c15,
            last_error: None,
        }
    }

    pub fn track_count(&self) -> usize {
        self.tracks.len()
    }

    pub fn current_title(&self) -> String {
        self.current
            .and_then(|index| self.tracks.get(index))
            .map_or_else(|| "（无音乐资源）".to_owned(), |path| track_title(path))
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn is_playing(&self) -> bool {
        self.enabled
            && self
                .music_player
                .as_ref()
                .is_some_and(|player| !player.is_paused() && !player.empty())
    }

    pub fn sound_enabled(&self) -> bool {
        self.sound_enabled
    }

    pub fn set_sound_enabled(&mut self, enabled: bool) {
        self.sound_enabled = enabled;
    }

    pub fn mode(&self) -> PlayMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: PlayMode) {
        self.mode = mode;
    }

    pub fn volume(&self) -> f32 {
        self.volume
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        if let Some(player) = &self.music_player {
            player.set_volume(self.volume);
        }
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        if enabled == self.enabled {
            return;
        }
        self.enabled = enabled;
        if enabled {
            if let Err(error) = self.play_current() {
                self.enabled = false;
                self.last_error = Some(error);
            }
        } else if let Some(player) = &self.music_player {
            player.stop();
        }
    }

    pub fn toggle_music(&mut self) {
        self.set_enabled(!self.enabled);
    }

    pub fn next_track(&mut self) {
        self.skip(1);
    }

    pub fn previous_track(&mut self) {
        self.skip(-1);
    }

    fn skip(&mut self, direction: i64) {
        if self.tracks.is_empty() {
            return;
        }
        let len = self.tracks.len();
        let current = self.current.unwrap_or(0);
        let next = if direction >= 0 {
            (current + 1) % len
        } else {
            (current + len - 1) % len
        };
        self.current = Some(next);
        if self.enabled
            && let Err(error) = self.play_current()
        {
            self.enabled = false;
            self.last_error = Some(error);
        }
    }

    /// Called once per frame: auto-advance when the current track ends.
    pub fn tick(&mut self) {
        if !self.enabled {
            return;
        }
        let finished = self.music_player.as_ref().is_some_and(rodio::Player::empty);
        if !finished {
            return;
        }
        self.shuffle_state = self
            .shuffle_state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let current = self.current.unwrap_or(0);
        if let Some(next) =
            next_track_index(self.mode, current, self.tracks.len(), self.shuffle_state)
        {
            self.current = Some(next);
            if let Err(error) = self.play_current() {
                self.enabled = false;
                self.last_error = Some(error);
            }
        }
    }

    pub fn play_turn_prompt(&mut self) {
        if !self.sound_enabled {
            return;
        }
        self.play_tones(&[(880.0, 120)]);
    }

    pub fn play_settlement_prompt(&mut self) {
        if !self.sound_enabled {
            return;
        }
        self.play_tones(&[(660.0, 120), (880.0, 160)]);
    }

    fn play_tones(&mut self, tones: &[(f32, u64)]) {
        if self.ensure_open().is_err() {
            return;
        }
        let Some(player) = &self.prompt_player else {
            return;
        };
        player.set_volume(0.5);
        for (frequency, millis) in tones {
            let tone = rodio::source::SineWave::new(*frequency)
                .take_duration(Duration::from_millis(*millis))
                .amplify(0.25);
            player.append(tone);
        }
    }

    fn play_current(&mut self) -> Result<(), String> {
        self.ensure_open()?;
        let Some(index) = self.current else {
            return Err("没有可播放的音乐文件".to_owned());
        };
        let Some(path) = self.tracks.get(index).cloned() else {
            return Err("音乐索引越界".to_owned());
        };
        let file =
            File::open(&path).map_err(|error| format!("open music {}: {error}", path.display()))?;
        let decoder = rodio::Decoder::try_from(BufReader::new(file))
            .map_err(|error| format!("decode music {}: {error}", path.display()))?;
        let Some(player) = &self.music_player else {
            return Err("音频设备未初始化".to_owned());
        };
        player.stop();
        player.append(decoder);
        player.set_volume(self.volume);
        player.play();
        Ok(())
    }

    fn ensure_open(&mut self) -> Result<(), String> {
        if self.sink.is_some() {
            return Ok(());
        }
        let sink = rodio::DeviceSinkBuilder::open_default_sink()
            .map_err(|error| format!("open default audio sink failed: {error}"))?;
        let music_player = rodio::Player::connect_new(sink.mixer());
        music_player.set_volume(self.volume);
        let prompt_player = rodio::Player::connect_new(sink.mixer());
        prompt_player.set_volume(0.5);
        self.music_player = Some(music_player);
        self.prompt_player = Some(prompt_player);
        self.sink = Some(sink);
        Ok(())
    }

    pub fn device_summary(&self) -> Option<String> {
        self.sink.as_ref().map(|sink| {
            let config = sink.config();
            format!(
                "{} Hz / {} ch / {:?}",
                config.sample_rate().get(),
                config.channel_count().get(),
                config.sample_format()
            )
        })
    }
}

impl Drop for AudioManager {
    fn drop(&mut self) {
        self.music_player.take();
        self.prompt_player.take();
        self.sink.take();
    }
}

/// Headless probe: decode the first music asset and print device information.
pub fn print_headless_probe() {
    println!("== poker-app audio probe (headless) ==");
    match decode_probe(3.0) {
        Ok(probe) => println!("decode: OK {}", probe.summary()),
        Err(error) => println!("decode: FAILED {error}"),
    }
    match rodio::DeviceSinkBuilder::open_default_sink() {
        Ok(sink) => {
            let config = sink.config();
            println!(
                "device: OK {} Hz / {} ch / {:?} / buffer {:?}",
                config.sample_rate().get(),
                config.channel_count().get(),
                config.sample_format(),
                config.buffer_size()
            );
        }
        Err(error) => println!("device: UNAVAILABLE {error}"),
    }
}

/// Headless playback probe: play the first music asset and verify the position.
pub fn play_headless_probe(seconds: f32) -> Result<(), String> {
    println!("== poker-app audio playback probe ==");
    let probe = decode_probe(2.0)?;
    println!("decode: OK {}", probe.summary());

    let sink = rodio::DeviceSinkBuilder::open_default_sink()
        .map_err(|error| format!("open default audio sink failed: {error}"))?;
    let config = sink.config();
    println!(
        "device: OK {} Hz / {} ch / {:?} / buffer {:?}",
        config.sample_rate().get(),
        config.channel_count().get(),
        config.sample_format(),
        config.buffer_size()
    );
    let (_, bytes) = probe_bytes()?;
    let decoder = rodio::Decoder::try_from(std::io::Cursor::new(bytes))
        .map_err(|error| format!("mp3 decode failed: {error}"))?;
    let player = rodio::Player::connect_new(sink.mixer());
    player.set_volume(0.35);
    player.append(decoder.repeat_infinite());
    player.play();

    let sleep = Duration::from_millis(200);
    let mut waited = 0.0_f32;
    while waited < seconds {
        std::thread::sleep(sleep);
        waited += sleep.as_secs_f32();
    }
    let position = player.get_pos().as_secs_f64();
    println!(
        "playback position after {waited:.1}s wall time: {position:.3}s (advancing={})",
        position > 0.1
    );
    player.stop();
    if position > 0.1 {
        Ok(())
    } else {
        Err(format!(
            "playback position did not advance (position={position:.3}s)"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playlist_modes_pick_expected_indices() {
        assert_eq!(next_track_index(PlayMode::Sequential, 0, 3, 0), Some(1));
        assert_eq!(next_track_index(PlayMode::Sequential, 2, 3, 0), Some(0));
        assert_eq!(next_track_index(PlayMode::RepeatOne, 1, 3, 123), Some(1));
        let shuffled = next_track_index(PlayMode::Shuffle, 1, 3, 42).expect("shuffle");
        assert_ne!(shuffled, 1);
        assert!(shuffled < 3);
        assert_eq!(next_track_index(PlayMode::Sequential, 0, 0, 0), None);
    }
}
