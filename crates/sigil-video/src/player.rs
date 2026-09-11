//! Play a file: pictures on the clock, sound out of the device.
//!
//! # Shape
//!
//! Two threads and a clock. The **sound** thread owns the output device and
//! keeps a ring of decoded samples ahead of it; the device's callback drains
//! the ring and counts what it played, and that count **is the clock** --
//! the picture on screen is whichever was due at the sound's position, so
//! the two cannot drift apart. A file with no sound gets a clock made of
//! wall time instead. The **picture** thread decodes ahead of the clock and
//! puts the picture due now where the interface can find it, then asks for
//! a repaint; the interface uploads a texture when the picture changed and
//! never decodes anything.
//!
//! Pausing stops the clock and the sound; seeking moves both decoders and
//! empties what was decoded ahead. Dropping the player stops the threads.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use crate::demux::{Demuxer, Description, Unplayable};
use crate::picture::{Ordered, Pictures};
use crate::sound::Sound;

/// What the interface reads.
pub struct Player {
    shared: Arc<Shared>,
    description: Description,
    _threads: Vec<std::thread::JoinHandle<()>>,
}

/// What the threads and the interface share.
struct Shared {
    /// Position, milliseconds, as of the last sound played or the last
    /// wall-clock tick.
    position_ms: AtomicU64,
    playing: AtomicBool,
    /// Volume as f32 bits; 0 is mute.
    volume: AtomicU32,
    stop: AtomicBool,
    /// A seek asked for and not yet done: the target, and a count so the
    /// threads can tell a new ask from the last.
    seek: Mutex<Option<u64>>,
    generation: AtomicU64,
    /// The picture due now, with its time, for the interface.
    frame: Mutex<Option<(u64, Arc<egui::ColorImage>)>>,
    /// Sound decoded ahead of the device, interleaved at the device's rate
    /// and channel count. Fed by the sound thread, drained by the device.
    ring: Mutex<std::collections::VecDeque<f32>>,
    /// Woken when the ring has room again.
    room: Condvar,
    /// Frames the device has played since the last seek, and where that
    /// seek was: together, the clock.
    played: AtomicU64,
    base_ms: AtomicU64,
    ended: AtomicBool,
    ctx: egui::Context,
    /// The sound thread's clock parameters, set once it has a device.
    rate: AtomicU32,
    has_sound: AtomicBool,
    /// For a file with no sound: when the current stretch of playing
    /// began, so the position is what was stored plus the time since.
    wall: Mutex<Option<Instant>>,
}

/// How far ahead of the device sound is decoded, in milliseconds.
const AHEAD_MS: u64 = 400;

impl Player {
    pub fn open(bytes: Arc<[u8]>, ctx: egui::Context) -> Result<Player, Unplayable> {
        Self::open_with(bytes, ctx, true)
    }

    /// `sound: false` plays the pictures on the wall clock and never
    /// touches the output device. For a test process: CoreAudio's first
    /// device query from inside `cargo test` takes ten to sixteen seconds
    /// on macOS (it waits on a main-thread run loop the harness never
    /// runs), where the same call from an application or an example takes
    /// fifty milliseconds. The sound path is covered by decoding the tone
    /// and by ear.
    pub fn open_with(
        bytes: Arc<[u8]>,
        ctx: egui::Context,
        sound: bool,
    ) -> Result<Player, Unplayable> {
        let demuxer = Demuxer::open(bytes.clone())?;
        let description = demuxer.description().clone();
        let pictures = Pictures::new(demuxer).map_err(Unplayable::Codec)?;
        let sound = if sound { Sound::open(bytes) } else { None };
        let shared = Arc::new(Shared {
            position_ms: AtomicU64::new(0),
            playing: AtomicBool::new(false),
            volume: AtomicU32::new(1.0f32.to_bits()),
            stop: AtomicBool::new(false),
            seek: Mutex::new(None),
            generation: AtomicU64::new(0),
            frame: Mutex::new(None),
            ring: Mutex::new(std::collections::VecDeque::new()),
            room: Condvar::new(),
            played: AtomicU64::new(0),
            base_ms: AtomicU64::new(0),
            ended: AtomicBool::new(false),
            ctx,
            rate: AtomicU32::new(0),
            has_sound: AtomicBool::new(sound.is_some()),
            wall: Mutex::new(None),
        });
        let mut threads = Vec::new();
        if let Some(sound) = sound {
            let s = shared.clone();
            threads.push(
                std::thread::Builder::new()
                    .name("video-sound".into())
                    .spawn(move || sound_thread(s, sound))
                    .expect("spawn"),
            );
        }
        let s = shared.clone();
        threads.push(
            std::thread::Builder::new()
                .name("video-pictures".into())
                .spawn(move || picture_thread(s, Ordered::new(pictures)))
                .expect("spawn"),
        );
        Ok(Player {
            shared,
            description,
            _threads: threads,
        })
    }

    pub fn description(&self) -> &Description {
        &self.description
    }

    /// The picture due now and its time, if one has been decoded.
    pub fn frame(&self) -> Option<(u64, Arc<egui::ColorImage>)> {
        self.shared.frame.lock().unwrap().clone()
    }

    pub fn position_ms(&self) -> u64 {
        self.shared.position()
    }

    pub fn duration_ms(&self) -> u64 {
        self.description.duration_ms
    }

    pub fn playing(&self) -> bool {
        self.shared.playing.load(Ordering::Relaxed)
    }

    pub fn ended(&self) -> bool {
        self.shared.ended.load(Ordering::Relaxed)
    }

    pub fn play(&self) {
        if self.ended() {
            self.seek(0);
        }
        self.shared.playing.store(true, Ordering::Relaxed);
        self.shared.mark_wall();
        self.shared.room.notify_all();
    }

    pub fn pause(&self) {
        self.shared.tick_wall();
        self.shared.playing.store(false, Ordering::Relaxed);
    }

    pub fn toggle(&self) {
        if self.playing() {
            self.pause();
        } else {
            self.play();
        }
    }

    pub fn seek(&self, ms: u64) {
        let ms = ms.min(self.description.duration_ms);
        *self.shared.seek.lock().unwrap() = Some(ms);
        self.shared.generation.fetch_add(1, Ordering::Relaxed);
        self.shared.position_ms.store(ms, Ordering::Relaxed);
        if self.playing() {
            self.shared.mark_wall();
        }
        self.shared.ended.store(false, Ordering::Relaxed);
        self.shared.room.notify_all();
    }

    pub fn volume(&self) -> f32 {
        f32::from_bits(self.shared.volume.load(Ordering::Relaxed))
    }

    pub fn set_volume(&self, v: f32) {
        self.shared
            .volume
            .store(v.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        self.shared.room.notify_all();
    }
}

impl Shared {
    /// Where playback is, in milliseconds: the sound's clock when there is
    /// sound, wall time otherwise.
    fn position(&self) -> u64 {
        if self.has_sound.load(Ordering::Relaxed) {
            // Until the device is up, nothing has played and the position
            // is where the last seek left it -- not the wall clock, which
            // would run the pictures ahead of sound that has not started.
            let rate = self.rate.load(Ordering::Relaxed) as u64;
            return match rate {
                0 => self.base_ms.load(Ordering::Relaxed),
                _ => {
                    self.base_ms.load(Ordering::Relaxed)
                        + self.played.load(Ordering::Relaxed) * 1000 / rate
                }
            };
        }
        if self.playing.load(Ordering::Relaxed) {
            let since = self
                .wall
                .lock()
                .unwrap()
                .map(|t| t.elapsed().as_millis() as u64)
                .unwrap_or(0);
            return self.position_ms.load(Ordering::Relaxed) + since;
        }
        self.position_ms.load(Ordering::Relaxed)
    }

    /// Wall-clock bookkeeping for a file with no sound: `mark` starts a
    /// stretch of playing, `tick` folds it into the position.
    fn mark_wall(&self) {
        *self.wall.lock().unwrap() = Some(Instant::now());
    }

    fn tick_wall(&self) {
        let pos = self.position();
        self.position_ms.store(pos, Ordering::Relaxed);
        *self.wall.lock().unwrap() = None;
    }
}

fn picture_thread(shared: Arc<Shared>, mut pictures: Ordered) {
    let mut seen = shared.generation.load(Ordering::Relaxed);
    let mut next = pictures.decode_next();
    let mut showing: Option<u64> = None;
    // The first picture straight away, so a video opened and not yet
    // playing shows its first frame rather than nothing.
    if let Some(f) = &next {
        *shared.frame.lock().unwrap() = Some((f.at_ms, Arc::new(f.image.clone())));
        showing = Some(f.at_ms);
        shared.ctx.request_repaint();
    }
    while !shared.stop.load(Ordering::Relaxed) {
        let generation = shared.generation.load(Ordering::Relaxed);
        if generation != seen {
            seen = generation;
            if let Some(ms) = shared.seek.lock().unwrap().take() {
                pictures.seek(ms);
                next = pictures.decode_next();
                // Up to the target, so what is shown is the picture at
                // the seek and not the keyframe before it.
                while let Some(f) = &next {
                    if f.at_ms >= ms {
                        break;
                    }
                    next = pictures.decode_next();
                }
                if let Some(f) = &next {
                    *shared.frame.lock().unwrap() = Some((f.at_ms, Arc::new(f.image.clone())));
                    showing = Some(f.at_ms);
                    shared.ctx.request_repaint();
                }
            }
        }
        if !shared.playing.load(Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(20));
            continue;
        }
        let now = shared.position();
        // Every picture due by now, keeping the last: a machine that fell
        // behind drops pictures rather than slowing down.
        let mut due = None;
        while let Some(f) = &next {
            if f.at_ms > now {
                break;
            }
            due = next.take();
            next = pictures.decode_next();
        }
        match (due, &next) {
            (Some(f), _) => {
                if showing != Some(f.at_ms) {
                    *shared.frame.lock().unwrap() = Some((f.at_ms, Arc::new(f.image)));
                    showing = Some(f.at_ms);
                    shared.ctx.request_repaint();
                }
            }
            (None, None) => {
                // The end. A file with sound ends when the sound does;
                // without, this is where it stops.
                if !shared.has_sound.load(Ordering::Relaxed) {
                    shared.tick_wall();
                    shared.playing.store(false, Ordering::Relaxed);
                    shared.ended.store(true, Ordering::Relaxed);
                    shared.ctx.request_repaint();
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            (None, Some(f)) => {
                let wait = f.at_ms.saturating_sub(now).min(50);
                std::thread::sleep(Duration::from_millis(wait.max(1)));
            }
        }
    }
}

fn sound_thread(shared: Arc<Shared>, mut sound: Sound) {
    let host = cpal::default_host();
    // No device, or one that will not open: play silent, on the wall clock
    // from wherever the position is now.
    let silent = |shared: &Shared| {
        shared
            .position_ms
            .store(shared.base_ms.load(Ordering::Relaxed), Ordering::Relaxed);
        shared.has_sound.store(false, Ordering::Relaxed);
        if shared.playing.load(Ordering::Relaxed) {
            shared.mark_wall();
        }
    };
    let Some(device) = host.default_output_device() else {
        return silent(&shared);
    };
    let Ok(supported) = device.default_output_config() else {
        return silent(&shared);
    };
    let config = supported.config();
    let rate = config.sample_rate;
    let channels = config.channels as usize;
    shared.rate.store(rate, Ordering::Relaxed);

    let s = shared.clone();
    let stream = device.build_output_stream(
        config,
        move |out: &mut [f32], _| {
            let playing = s.playing.load(Ordering::Relaxed);
            let volume = f32::from_bits(s.volume.load(Ordering::Relaxed));
            let mut ring = s.ring.lock().unwrap();
            let mut n = 0;
            for sample in out.iter_mut() {
                *sample = if playing {
                    match ring.pop_front() {
                        Some(v) => {
                            n += 1;
                            v * volume
                        }
                        None => 0.0,
                    }
                } else {
                    0.0
                };
            }
            drop(ring);
            if n > 0 {
                s.played.fetch_add((n / channels) as u64, Ordering::Relaxed);
                s.room.notify_all();
            }
        },
        |_| {},
        None,
    );
    let Ok(stream) = stream else {
        return silent(&shared);
    };
    let _ = stream.play();

    let ahead = (rate as u64 * AHEAD_MS / 1000) as usize * channels;
    let mut seen = shared.generation.load(Ordering::Relaxed);
    let mut carry: Vec<f32> = Vec::new();
    let mut done = false;
    while !shared.stop.load(Ordering::Relaxed) {
        let generation = shared.generation.load(Ordering::Relaxed);
        if generation != seen {
            seen = generation;
            let target = shared.seek.lock().unwrap().unwrap_or(0);
            sound.seek(target);
            shared.ring.lock().unwrap().clear();
            carry.clear();
            shared.played.store(0, Ordering::Relaxed);
            shared.base_ms.store(target, Ordering::Relaxed);
            done = false;
        }
        let filled = shared.ring.lock().unwrap().len();
        if filled >= ahead || done {
            // Wait for room, or a seek, or a stop -- not a spin.
            let guard = shared.ring.lock().unwrap();
            let _ = shared
                .room
                .wait_timeout(guard, Duration::from_millis(50))
                .unwrap();
            if done && shared.playing.load(Ordering::Relaxed) && filled == 0 {
                shared.playing.store(false, Ordering::Relaxed);
                shared.ended.store(true, Ordering::Relaxed);
                shared.ctx.request_repaint();
            }
            continue;
        }
        match sound.decode_next() {
            Some(chunk) => {
                let fitted = fit(
                    &chunk.samples,
                    sound.channels,
                    sound.rate,
                    channels,
                    rate,
                    &mut carry,
                );
                shared.ring.lock().unwrap().extend(fitted);
            }
            None => done = true,
        }
    }
}

/// Sound as the device wants it: its channel count and its rate.
///
/// Linear resampling, which is enough for speech and music under a
/// picture; `carry` keeps the fraction of a frame between chunks so the
/// seam does not click.
fn fit(
    samples: &[f32],
    from_channels: usize,
    from_rate: u32,
    to_channels: usize,
    to_rate: u32,
    carry: &mut Vec<f32>,
) -> Vec<f32> {
    // Channels first.
    let frames = samples.len() / from_channels.max(1);
    let mut mapped = Vec::with_capacity(frames * to_channels);
    for i in 0..frames {
        for c in 0..to_channels {
            let src = if from_channels == 1 {
                0
            } else {
                c.min(from_channels - 1)
            };
            mapped.push(samples[i * from_channels + src]);
        }
    }
    if from_rate == to_rate {
        return mapped;
    }
    // Then the rate, with the previous chunk's last frame in front so the
    // interpolation crosses the seam.
    let mut input = std::mem::take(carry);
    let lead = input.len() / to_channels;
    input.extend_from_slice(&mapped);
    let in_frames = input.len() / to_channels;
    let ratio = from_rate as f64 / to_rate as f64;
    let out_frames = ((in_frames.saturating_sub(lead)) as f64 / ratio) as usize;
    let mut out = Vec::with_capacity(out_frames * to_channels);
    for o in 0..out_frames {
        let pos = o as f64 * ratio;
        let i = pos.floor() as usize;
        let t = (pos - i as f64) as f32;
        let j = (i + 1).min(in_frames - 1);
        for c in 0..to_channels {
            let a = input[i * to_channels + c];
            let b = input[j * to_channels + c];
            out.push(a + (b - a) * t);
        }
    }
    if in_frames > 0 {
        carry.extend_from_slice(&input[(in_frames - 1) * to_channels..]);
    }
    out
}
