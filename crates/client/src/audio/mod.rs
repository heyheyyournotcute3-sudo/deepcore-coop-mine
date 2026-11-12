//! Procedural audio: every sound is synthesised at startup, so the game ships with no audio
//! assets. Digging, placing, machine hums, the reactor alarm and Geiger clicks.

use std::num::{NonZeroU16, NonZeroU32};

use bevy::audio::{AddAudioSource, Decodable, Sample};
use bevy::prelude::*;
use bevy::audio::Source;

use crate::net::SessionState;

/// Registers the procedural sounds.
pub struct AudioPlugin;

impl Plugin for AudioPlugin {
    fn build(&self, app: &mut App) {
        app.add_audio_source::<ProceduralSound>()
            .init_resource::<SoundLibrary>()
            .add_systems(Startup, build_library)
            .add_systems(Update, (play_mining_sound, play_geiger_clicks, play_notifications));
    }
}

/// The kinds of synthesised sound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundKind {
    /// A pickaxe hitting rock.
    Dig,
    /// A block being placed.
    Place,
    /// A machine running.
    MachineHum,
    /// The reactor alarm.
    Alarm,
    /// A Geiger counter click.
    GeigerClick,
}

/// A generated sound asset.
#[derive(Debug, Asset, TypePath)]
pub struct ProceduralSound {
    /// Mono samples.
    pub samples: Vec<f32>,
    /// Sample rate.
    pub sample_rate: u32,
}

impl ProceduralSound {
    /// Generates a sound.
    pub fn generate(kind: SoundKind, sample_rate: u32) -> Self {
        let duration = match kind {
            SoundKind::Dig => 0.18,
            SoundKind::Place => 0.12,
            SoundKind::MachineHum => 1.0,
            SoundKind::Alarm => 0.8,
            SoundKind::GeigerClick => 0.04,
        };
        let count = (duration * sample_rate as f32) as usize;
        let mut samples = Vec::with_capacity(count);
        let mut noise_state: u32 = 0x1234_5678;
        for index in 0..count {
            let t = index as f32 / sample_rate as f32;
            let progress = t / duration;
            let sample = match kind {
                SoundKind::Dig => {
                    // Filtered noise burst plus a low thump.
                    noise_state = noise_state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    let noise = (noise_state >> 8) as f32 / 8_388_608.0 - 1.0;
                    let thump = (t * 90.0 * std::f32::consts::TAU).sin() * 0.5;
                    (noise * 0.35 + thump) * (1.0 - progress).powi(2)
                }
                SoundKind::Place => {
                    noise_state = noise_state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    let noise = (noise_state >> 8) as f32 / 8_388_608.0 - 1.0;
                    noise * 0.3 * (1.0 - progress).powi(3)
                }
                SoundKind::MachineHum => {
                    let base = (t * 70.0 * std::f32::consts::TAU).sin() * 0.12;
                    let overtone = (t * 140.0 * std::f32::consts::TAU).sin() * 0.05;
                    (base + overtone) * 0.6
                }
                SoundKind::Alarm => {
                    let sweep = if progress < 0.5 { 440.0 } else { 620.0 };
                    (t * sweep * std::f32::consts::TAU).sin() * 0.25
                }
                SoundKind::GeigerClick => {
                    noise_state = noise_state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    let noise = (noise_state >> 8) as f32 / 8_388_608.0 - 1.0;
                    noise * (1.0 - progress).powi(4) * 0.6
                }
            };
            samples.push(sample.clamp(-1.0, 1.0));
        }
        Self {
            samples,
            sample_rate,
        }
    }
}

/// A rodio decoder over the generated samples.
pub struct ProceduralDecoder {
    samples: std::vec::IntoIter<f32>,
    sample_rate: u32,
}

impl Iterator for ProceduralDecoder {
    type Item = Sample;

    fn next(&mut self) -> Option<Sample> {
        self.samples.next()
    }
}

impl Source for ProceduralDecoder {
    fn current_span_len(&self) -> Option<usize> {
        let remaining = self.samples.as_slice().len();
        Some(remaining.max(1))
    }

    fn channels(&self) -> NonZeroU16 {
        NonZeroU16::new(1).expect("one channel is always valid")
    }

    fn sample_rate(&self) -> NonZeroU32 {
        NonZeroU32::new(self.sample_rate).expect("sample rate is non-zero")
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        Some(std::time::Duration::from_secs_f32(
            self.samples.as_slice().len() as f32 / self.sample_rate as f32,
        ))
    }
}

impl Decodable for ProceduralSound {
    type Decoder = ProceduralDecoder;

    fn decoder(&self) -> Self::Decoder {
        ProceduralDecoder {
            samples: self.samples.clone().into_iter(),
            sample_rate: self.sample_rate,
        }
    }
}

/// Handles to every generated sound.
#[derive(Debug, Resource, Default)]
pub struct SoundLibrary {
    /// Dig.
    pub dig: Handle<ProceduralSound>,
    /// Place.
    pub place: Handle<ProceduralSound>,
    /// Machine hum.
    pub hum: Handle<ProceduralSound>,
    /// Alarm.
    pub alarm: Handle<ProceduralSound>,
    /// Geiger click.
    pub geiger: Handle<ProceduralSound>,
    /// Seconds since the last dig sound.
    pub dig_cooldown: f32,
}

fn build_library(mut commands: Commands, mut sounds: ResMut<Assets<ProceduralSound>>) {
    let library = SoundLibrary {
        dig: sounds.add(ProceduralSound::generate(SoundKind::Dig, 44_100)),
        place: sounds.add(ProceduralSound::generate(SoundKind::Place, 44_100)),
        hum: sounds.add(ProceduralSound::generate(SoundKind::MachineHum, 44_100)),
        alarm: sounds.add(ProceduralSound::generate(SoundKind::Alarm, 44_100)),
        geiger: sounds.add(ProceduralSound::generate(SoundKind::GeigerClick, 44_100)),
        dig_cooldown: 0.0,
    };
    commands.insert_resource(library);
}

fn play_mining_sound(
    time: Res<Time>,
    mut library: ResMut<SoundLibrary>,
    prediction: Res<crate::prediction::PredictionState>,
    mut commands: Commands,
) {
    library.dig_cooldown -= time.delta_secs();
    if prediction.mining.is_some() && library.dig_cooldown <= 0.0 {
        library.dig_cooldown = 0.25;
        commands.spawn((
            AudioPlayer::<ProceduralSound>(library.dig.clone()),
            PlaybackSettings::DESPAWN.with_volume(bevy::audio::Volume::Linear(0.35)),
        ));
    }
}

fn play_geiger_clicks(
    time: Res<Time>,
    session: Res<SessionState>,
    library: Res<SoundLibrary>,
    mut commands: Commands,
    mut timer: Local<f32>,
) {
    let rate = deepcore_shared::hazards::geiger_rate(session.dose_rate());
    if rate <= 0.0 {
        return;
    }
    *timer += time.delta_secs();
    let interval = 1.0 / rate.max(0.5);
    if *timer >= interval {
        *timer = 0.0;
        commands.spawn((
            AudioPlayer::<ProceduralSound>(library.geiger.clone()),
            PlaybackSettings::DESPAWN.with_volume(bevy::audio::Volume::Linear(0.25)),
        ));
    }
}

fn play_notifications(
    session: Res<SessionState>,
    library: Res<SoundLibrary>,
    mut commands: Commands,
    mut last_seen: Local<usize>,
) {
    if session.notifications.len() > *last_seen {
        let severity = session
            .notifications
            .last()
            .map(|(_, severity, _)| *severity)
            .unwrap_or(0);
        *last_seen = session.notifications.len();
        let handle = if severity >= 2 {
            library.alarm.clone()
        } else {
            library.place.clone()
        };
        commands.spawn((
            AudioPlayer::<ProceduralSound>(handle),
            PlaybackSettings::DESPAWN.with_volume(bevy::audio::Volume::Linear(0.2)),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_sounds_are_audible_and_clipped() {
        for kind in [
            SoundKind::Dig,
            SoundKind::Place,
            SoundKind::MachineHum,
            SoundKind::Alarm,
            SoundKind::GeigerClick,
        ] {
            let sound = ProceduralSound::generate(kind, 44_100);
            assert!(!sound.samples.is_empty(), "{kind:?} produced no samples");
            assert!(
                sound.samples.iter().any(|sample| sample.abs() > 0.01),
                "{kind:?} is silent"
            );
            assert!(sound.samples.iter().all(|sample| sample.abs() <= 1.0));
        }
    }
}
