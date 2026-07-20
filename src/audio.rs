use glam::Vec3;
use rodio::{OutputStream, OutputStreamHandle, Sink, SpatialSink};
use std::collections::HashMap;
use std::fs::{create_dir_all, File};
use std::io::{Cursor, Write};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SoundMaterial {
    Grass,
    Wood,
    Sand,
    Gravel,
    Stone,
    Snow,
    Ice,
    Glass,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SoundId {
    BlockBreak(SoundMaterial),
    BlockPlace(SoundMaterial),
    Footstep(SoundMaterial),
    Jump,
    Land(SoundMaterial),
    PlayerHurt,
    PlayerDeath,
    UiClick,
    CreeperIgnition,
    Explosion,
    ArrowShoot,
    Note(u8),
}

impl SoundId {
    pub fn filename(&self) -> String {
        match self {
            SoundId::BlockBreak(m) => format!("{:?}_break.wav", m).to_lowercase(),
            SoundId::BlockPlace(m) => format!("{:?}_place.wav", m).to_lowercase(),
            SoundId::Footstep(m) => format!("{:?}_step.wav", m).to_lowercase(),
            SoundId::Jump => "jump.wav".to_string(),
            SoundId::Land(m) => format!("{:?}_land.wav", m).to_lowercase(),
            SoundId::PlayerHurt => "hurt.wav".to_string(),
            SoundId::PlayerDeath => "death.wav".to_string(),
            SoundId::UiClick => "click.wav".to_string(),
            SoundId::CreeperIgnition => "creeper_hiss.wav".to_string(),
            SoundId::Explosion => "explosion.wav".to_string(),
            SoundId::ArrowShoot => "bow_shoot.wav".to_string(),
            SoundId::Note(note) => format!("note_{note}.wav"),
        }
    }
}

pub struct AudioManager {
    _stream: Option<OutputStream>,
    stream_handle: Option<OutputStreamHandle>,
    pub volume: f32,
    sound_cache: HashMap<SoundId, Vec<u8>>,
    active_loops: HashMap<u64, Sink>,
}

fn create_wav_bytes(samples: &[f32], sample_rate: u32) -> Vec<u8> {
    let mut writer = Vec::new();
    writer.extend_from_slice(b"RIFF");
    let file_size = 36 + samples.len() * 2;
    writer.extend_from_slice(&(file_size as u32).to_le_bytes());
    writer.extend_from_slice(b"WAVE");
    writer.extend_from_slice(b"fmt ");
    writer.extend_from_slice(&(16u32).to_le_bytes());
    writer.extend_from_slice(&(1u16).to_le_bytes());
    writer.extend_from_slice(&(1u16).to_le_bytes());
    writer.extend_from_slice(&(sample_rate).to_le_bytes());
    let byte_rate = sample_rate * 2;
    writer.extend_from_slice(&(byte_rate).to_le_bytes());
    writer.extend_from_slice(&(2u16).to_le_bytes());
    writer.extend_from_slice(&(16u16).to_le_bytes());
    writer.extend_from_slice(b"data");
    let data_size = samples.len() * 2;
    writer.extend_from_slice(&(data_size as u32).to_le_bytes());
    for &s in samples {
        let clamped = s.clamp(-1.0, 1.0);
        let sample_i16 = (clamped * 32767.0) as i16;
        writer.extend_from_slice(&sample_i16.to_le_bytes());
    }
    writer
}

fn synth_noise(duration: f32, sample_rate: u32, mut seed: u32) -> Vec<f32> {
    let len = (duration * sample_rate as f32) as usize;
    let mut samples = Vec::with_capacity(len);
    for _ in 0..len {
        seed = seed.wrapping_mul(1103515245).wrapping_add(12345);
        let n = (seed as f32 / 4294967296.0) * 2.0 - 1.0;
        samples.push(n);
    }
    samples
}

fn synth_sound(sound_id: SoundId) -> Vec<f32> {
    let sample_rate = 22050;
    let seed = 54321;
    match sound_id {
        SoundId::UiClick => {
            let len = (0.05 * sample_rate as f32) as usize;
            (0..len)
                .map(|i| {
                    let t = i as f32 / sample_rate as f32;
                    let env = 1.0 - (t / 0.05);
                    (2.0 * std::f32::consts::PI * 1000.0 * t).sin() * env * 0.5
                })
                .collect()
        }
        SoundId::PlayerHurt => {
            let len = (0.15 * sample_rate as f32) as usize;
            (0..len)
                .map(|i| {
                    let t = i as f32 / sample_rate as f32;
                    let env = (1.0 - (t / 0.15)).powi(2);
                    let freq = 180.0 - (t / 0.15) * 100.0;
                    let tri = 2.0 * ((t * freq).fract() - 0.5).abs() - 0.5;
                    tri * env * 0.8
                })
                .collect()
        }
        SoundId::PlayerDeath => {
            let len = (0.4 * sample_rate as f32) as usize;
            (0..len)
                .map(|i| {
                    let t = i as f32 / sample_rate as f32;
                    let env = (1.0 - (t / 0.4)).powi(2);
                    let freq = 120.0 - (t / 0.4) * 80.0;
                    let tri = 2.0 * ((t * freq).fract() - 0.5).abs() - 0.5;
                    tri * env * 0.8
                })
                .collect()
        }
        SoundId::ArrowShoot => {
            let noise = synth_noise(0.12, sample_rate, seed);
            noise
                .into_iter()
                .enumerate()
                .map(|(i, val)| {
                    let t = i as f32 / sample_rate as f32;
                    let env = (1.0 - (t / 0.12)).powi(3);
                    val * env * 0.3
                })
                .collect()
        }
        SoundId::Note(note) => {
            let duration = 0.35;
            let frequency = 440.0 * 2.0f32.powf((note as f32 - 12.0) / 12.0);
            let len = (duration * sample_rate as f32) as usize;
            (0..len)
                .map(|i| {
                    let t = i as f32 / sample_rate as f32;
                    let env = (1.0 - t / duration).max(0.0).powi(2);
                    (2.0 * std::f32::consts::PI * frequency * t).sin() * env * 0.45
                })
                .collect()
        }
        SoundId::Explosion => {
            let len = (1.5 * sample_rate as f32) as usize;
            let raw_noise = synth_noise(1.5, sample_rate, seed);
            let mut filtered = vec![0.0; len];
            let mut prev = 0.0;
            for i in 0..len {
                let t = i as f32 / sample_rate as f32;
                let env = (1.0 - (t / 1.5)).powi(2);
                prev = prev * 0.90 + raw_noise[i] * 0.10;
                filtered[i] = prev * env * 0.8;
            }
            filtered
        }
        SoundId::CreeperIgnition => {
            let len = (1.5 * sample_rate as f32) as usize;
            let raw_noise = synth_noise(1.5, sample_rate, seed);
            let mut filtered = vec![0.0; len];
            let mut prev = 0.0;
            for i in 0..len {
                prev = prev * 0.3 + raw_noise[i] * 0.7; // high pass filter focus
                filtered[i] = prev * 0.4;
            }
            filtered
        }
        SoundId::Jump => {
            let noise = synth_noise(0.10, sample_rate, seed);
            noise
                .into_iter()
                .enumerate()
                .map(|(i, val)| {
                    let t = i as f32 / sample_rate as f32;
                    let env = (1.0 - (t / 0.10)).powi(2);
                    val * env * 0.15
                })
                .collect()
        }
        SoundId::Footstep(mat)
        | SoundId::BlockBreak(mat)
        | SoundId::BlockPlace(mat)
        | SoundId::Land(mat) => {
            let dur = match sound_id {
                SoundId::BlockBreak(_) => 0.22,
                SoundId::BlockPlace(_) => 0.12,
                SoundId::Land(_) => 0.20,
                _ => 0.14,
            };
            let len = (dur * sample_rate as f32) as usize;
            let raw_noise = synth_noise(dur, sample_rate, seed);
            let filter_coef = match mat {
                SoundMaterial::Grass => 0.94,
                SoundMaterial::Wood => 0.70,
                SoundMaterial::Sand => 0.40,
                SoundMaterial::Gravel => 0.50,
                SoundMaterial::Stone => 0.85,
                SoundMaterial::Snow => 0.97,
                SoundMaterial::Ice => 0.80,
                SoundMaterial::Glass => 0.20,
            };
            let amp = match mat {
                SoundMaterial::Grass => 0.15,
                SoundMaterial::Wood => 0.25,
                SoundMaterial::Stone => 0.35,
                SoundMaterial::Sand | SoundMaterial::Gravel => 0.20,
                _ => 0.20,
            };
            let mut filtered = vec![0.0; len];
            let mut prev = 0.0;
            for i in 0..len {
                let t = i as f32 / sample_rate as f32;
                let env = 1.0 - (t / dur);
                prev = prev * filter_coef + raw_noise[i] * (1.0 - filter_coef);
                filtered[i] = prev * env * amp;
            }
            filtered
        }
    }
}

impl AudioManager {
    pub fn new() -> Self {
        let (_stream, stream_handle) = match OutputStream::try_default() {
            Ok((s, h)) => (Some(s), Some(h)),
            Err(e) => {
                eprintln!(
                    "[Audio] Warning: Failed to open default audio output device: {:?}",
                    e
                );
                (None, None)
            }
        };

        let sound_dir = Path::new("assets/sounds");
        if !sound_dir.exists() {
            let _ = create_dir_all(sound_dir);
        }

        let mut sound_cache = HashMap::new();
        let sound_ids = vec![
            SoundId::Jump,
            SoundId::PlayerHurt,
            SoundId::PlayerDeath,
            SoundId::UiClick,
            SoundId::CreeperIgnition,
            SoundId::Explosion,
            SoundId::ArrowShoot,
            SoundId::BlockBreak(SoundMaterial::Grass),
            SoundId::BlockBreak(SoundMaterial::Wood),
            SoundId::BlockBreak(SoundMaterial::Stone),
            SoundId::BlockBreak(SoundMaterial::Sand),
            SoundId::BlockBreak(SoundMaterial::Gravel),
            SoundId::BlockBreak(SoundMaterial::Snow),
            SoundId::BlockBreak(SoundMaterial::Ice),
            SoundId::BlockBreak(SoundMaterial::Glass),
            SoundId::BlockPlace(SoundMaterial::Grass),
            SoundId::BlockPlace(SoundMaterial::Wood),
            SoundId::BlockPlace(SoundMaterial::Stone),
            SoundId::BlockPlace(SoundMaterial::Sand),
            SoundId::BlockPlace(SoundMaterial::Gravel),
            SoundId::BlockPlace(SoundMaterial::Snow),
            SoundId::BlockPlace(SoundMaterial::Ice),
            SoundId::BlockPlace(SoundMaterial::Glass),
            SoundId::Footstep(SoundMaterial::Grass),
            SoundId::Footstep(SoundMaterial::Wood),
            SoundId::Footstep(SoundMaterial::Stone),
            SoundId::Footstep(SoundMaterial::Sand),
            SoundId::Footstep(SoundMaterial::Gravel),
            SoundId::Footstep(SoundMaterial::Snow),
            SoundId::Footstep(SoundMaterial::Ice),
            SoundId::Footstep(SoundMaterial::Glass),
            SoundId::Land(SoundMaterial::Grass),
            SoundId::Land(SoundMaterial::Wood),
            SoundId::Land(SoundMaterial::Stone),
            SoundId::Land(SoundMaterial::Sand),
            SoundId::Land(SoundMaterial::Gravel),
            SoundId::Land(SoundMaterial::Snow),
            SoundId::Land(SoundMaterial::Ice),
            SoundId::Land(SoundMaterial::Glass),
        ];

        let sound_ids = sound_ids.into_iter().chain((0..25).map(SoundId::Note));

        for id in sound_ids {
            let filename = id.filename();
            let file_path = sound_dir.join(&filename);
            let mut loaded_bytes = Vec::new();
            let mut successfully_loaded = false;

            if file_path.exists() {
                if let Ok(mut f) = File::open(&file_path) {
                    use std::io::Read;
                    if f.read_to_end(&mut loaded_bytes).is_ok() {
                        successfully_loaded = true;
                    }
                }
            }

            if !successfully_loaded {
                let samples = synth_sound(id);
                let wav_bytes = create_wav_bytes(&samples, 22050);
                // Note-block pitches are cheap procedural variants; keep them
                // in memory instead of creating 25 generated files per world.
                if !matches!(id, SoundId::Note(_)) {
                    if let Ok(mut f) = File::create(&file_path) {
                        let _ = f.write_all(&wav_bytes);
                    }
                }
                loaded_bytes = wav_bytes;
            }

            sound_cache.insert(id, loaded_bytes);
        }

        Self {
            _stream,
            stream_handle,
            volume: 1.0,
            sound_cache,
            active_loops: HashMap::new(),
        }
    }

    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        for sink in self.active_loops.values() {
            sink.set_volume(self.volume);
        }
    }

    fn get_source(&self, sound_id: SoundId) -> Option<rodio::Decoder<Cursor<Vec<u8>>>> {
        let bytes = self.sound_cache.get(&sound_id)?.clone();
        rodio::Decoder::new(Cursor::new(bytes)).ok()
    }

    pub fn play_sound(&self, sound_id: SoundId) {
        let handle = match &self.stream_handle {
            Some(h) => h,
            None => return,
        };
        if let Some(source) = self.get_source(sound_id) {
            if let Ok(sink) = Sink::try_new(handle) {
                sink.set_volume(self.volume);
                sink.append(source);
                sink.detach();
            }
        }
    }

    pub fn play_sound_3d(
        &self,
        sound_id: SoundId,
        pos: Vec3,
        listener_pos: Vec3,
        listener_right: Vec3,
    ) {
        let handle = match &self.stream_handle {
            Some(h) => h,
            None => return,
        };
        if let Some(source) = self.get_source(sound_id) {
            let left_ear = listener_pos - listener_right * 0.15;
            let right_ear = listener_pos + listener_right * 0.15;
            if let Ok(sink) = SpatialSink::try_new(
                handle,
                [pos.x, pos.y, pos.z],
                [left_ear.x, left_ear.y, left_ear.z],
                [right_ear.x, right_ear.y, right_ear.z],
            ) {
                sink.set_volume(self.volume);
                sink.append(source);
                sink.detach();
            }
        }
    }

    pub fn start_looping_sound(&mut self, entity_id: u64, sound_id: SoundId, _pos: Vec3) {
        let handle = match &self.stream_handle {
            Some(h) => h,
            None => return,
        };
        if let Some(source) = self.get_source(sound_id) {
            if let Ok(sink) = Sink::try_new(handle) {
                sink.set_volume(self.volume);
                sink.append(source);
                self.active_loops.insert(entity_id, sink);
            }
        }
    }

    pub fn update_looping_sound_position(
        &self,
        _entity_id: u64,
        _pos: Vec3,
        _listener_pos: Vec3,
        _listener_right: Vec3,
    ) {
    }

    pub fn stop_looping_sound(&mut self, entity_id: u64) {
        if let Some(sink) = self.active_loops.remove(&entity_id) {
            sink.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_wav_synthesis() {
        let samples = vec![0.0, 0.5, -0.5, 0.0];
        let wav = create_wav_bytes(&samples, 22050);
        assert!(wav.starts_with(b"RIFF"));
        assert_eq!(&wav[8..12], b"WAVE");
    }
}
