//! A voice note: Ogg-encapsulated Opus, decoded here and played.
//!
//! # Why this is not the video player
//!
//! SIP-18 fixes the format — "Ogg-encapsulated Opus (RFC 7845), mono,
//! RECOMMENDED 24 kbit/s" — and says what a voice note is *not*: "a file
//! and not a stream: it carries no SIP-15 framing, no timestamps and no
//! comfort frames." So there is nothing to demux against a clock and
//! nothing to keep in step with a picture. A note is seconds long; it is
//! decoded whole, once, and played from memory.
//!
//! The container is read here rather than by symphonia because symphonia
//! has no Opus decoder — the packets would have to be handed to libopus
//! anyway — and an Ogg page is a header, a segment table and the bytes.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

/// Opus always decodes at one of a few rates; 48 kHz is the one RFC 7845
/// defines the granule position and the pre-skip in, so it is the one this
/// asks for whatever the sender recorded at.
pub const RATE: u32 = 48_000;

/// The longest note this will decode.
///
/// **The decode is synchronous**, on whichever thread pressed play, and
/// libopus runs at something like a hundred times real time: three minutes
/// is about thirty milliseconds, which is a frame nobody sees, and 8.6 MB
/// of `f32` to hold. A ten-minute file would be a visible stall for a
/// shape of audio nobody makes by holding a button down, so it is refused
/// instead -- and the refusal is read off the last page's granule, before
/// a byte is decoded.
const LONGEST_MS: u64 = 3 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unreadable {
    /// Not an Ogg stream at all, or not one carrying Opus.
    NotAVoiceNote,
    /// Ogg and Opus, and the audio will not decode.
    Broken(String),
    /// Longer than [`LONGEST_MS`].
    TooLong,
}

impl std::fmt::Display for Unreadable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unreadable::NotAVoiceNote => write!(f, "not an Ogg Opus voice note"),
            Unreadable::Broken(why) => write!(f, "{why}"),
            Unreadable::TooLong => write!(f, "too long to play"),
        }
    }
}

/// One Ogg page's worth of header, as RFC 3533 lays it out.
struct Page<'a> {
    header_type: u8,
    granule: u64,
    segments: &'a [u8],
    body: &'a [u8],
    /// Where the next page starts in the file.
    next: usize,
}

/// Read the page beginning at `at`, or `None` if there is not a whole one
/// there.
///
/// **No CRC check.** The blob arrived sealed and authenticated (SIP-18),
/// so a bit-flip in it is not a threat model this can add anything to; a
/// truncated or malformed page stops the read, which is the behaviour that
/// matters.
fn page(bytes: &[u8], at: usize) -> Option<Page<'_>> {
    const HEADER: usize = 27;
    let head = bytes.get(at..at + HEADER)?;
    if &head[0..4] != b"OggS" || head[4] != 0 {
        return None;
    }
    let granule = u64::from_le_bytes(head[6..14].try_into().ok()?);
    let count = head[26] as usize;
    let segments = bytes.get(at + HEADER..at + HEADER + count)?;
    let length: usize = segments.iter().map(|&s| s as usize).sum();
    let start = at + HEADER + count;
    let body = bytes.get(start..start + length)?;
    Some(Page {
        header_type: head[5],
        granule,
        segments,
        body,
        next: start + length,
    })
}

/// Every packet in the stream, in order, with the last page's granule.
///
/// A packet spans segments until one is shorter than 255, and may span
/// pages: a page whose header says "continued" carries the rest of the one
/// the page before it left open.
fn packets(bytes: &[u8]) -> Option<(Vec<Vec<u8>>, u64)> {
    let mut out: Vec<Vec<u8>> = Vec::new();
    let mut open: Vec<u8> = Vec::new();
    let mut continued = false;
    let mut at = 0usize;
    let mut granule = 0u64;
    while let Some(p) = page(bytes, at) {
        // A page that does not continue one abandons whatever was left
        // open: the stream is malformed and the half packet is not audio.
        if !(p.header_type & 0x01 != 0) && continued {
            open.clear();
        }
        let mut cut = 0usize;
        for &len in p.segments {
            let part = &p.body[cut..cut + len as usize];
            cut += len as usize;
            open.extend_from_slice(part);
            if len < 255 {
                out.push(std::mem::take(&mut open));
                continued = false;
            } else {
                continued = true;
            }
        }
        if p.granule != u64::MAX {
            granule = p.granule;
        }
        at = p.next;
    }
    if out.is_empty() {
        None
    } else {
        Some((out, granule))
    }
}

/// What the stream says about itself: RFC 7845's `OpusHead`.
struct Head {
    channels: usize,
    /// Samples at 48 kHz the encoder put in front and the player throws
    /// away. Not a nicety: without it every note begins with a click and
    /// runs a few milliseconds long.
    pre_skip: usize,
}

fn head(packet: &[u8]) -> Option<Head> {
    if packet.len() < 19 || &packet[0..8] != b"OpusHead" {
        return None;
    }
    let channels = packet[9] as usize;
    if channels == 0 || channels > 2 {
        return None;
    }
    Some(Head {
        channels,
        pre_skip: u16::from_le_bytes([packet[10], packet[11]]) as usize,
    })
}

/// A voice note, decoded: interleaved `f32` at [`RATE`].
#[derive(Debug)]
pub struct Decoded {
    pub samples: Vec<f32>,
    pub channels: usize,
}

impl Decoded {
    pub fn duration_ms(&self) -> u64 {
        let frames = self.samples.len() / self.channels.max(1);
        frames as u64 * 1000 / RATE as u64
    }
}

/// Decode a whole note.
pub fn decode(bytes: &[u8]) -> Result<Decoded, Unreadable> {
    let (packets, granule) = packets(bytes).ok_or(Unreadable::NotAVoiceNote)?;
    let head =
        head(packets.first().ok_or(Unreadable::NotAVoiceNote)?).ok_or(Unreadable::NotAVoiceNote)?;
    // The granule position of the last page is the end of the stream in
    // 48 kHz samples, pre-skip included. Checked before decoding, so a
    // file claiming to be a day long is refused rather than decoded.
    let claimed = granule.saturating_sub(head.pre_skip as u64) * 1000 / RATE as u64;
    if claimed > LONGEST_MS {
        return Err(Unreadable::TooLong);
    }
    let channels = match head.channels {
        1 => opus::Channels::Mono,
        _ => opus::Channels::Stereo,
    };
    let mut decoder = opus::Decoder::new(RATE, channels)
        .map_err(|e| Unreadable::Broken(format!("no decoder: {e}")))?;
    // 120 ms is the longest frame Opus has, and a packet may hold one.
    let most = (RATE as usize / 1000 * 120) * head.channels;
    let mut out: Vec<f32> = Vec::new();
    let mut frame = vec![0f32; most];
    // The first two packets are `OpusHead` and `OpusTags`, which are not
    // audio. Anything after them is.
    for packet in packets.iter().skip(2) {
        match decoder.decode_float(packet, &mut frame, false) {
            Ok(frames) => out.extend_from_slice(&frame[..frames * head.channels]),
            // One bad packet is a gap, not a dead note: keep what decoded.
            Err(_) => break,
        }
    }
    if out.is_empty() {
        return Err(Unreadable::Broken("no audio decoded".into()));
    }
    let skip = (head.pre_skip * head.channels).min(out.len());
    out.drain(..skip);
    Ok(Decoded {
        samples: out,
        channels: head.channels,
    })
}

/// RFC 3533's CRC32 over a whole page: polynomial `0x04c11db7`, no
/// reflection, zero init, no final xor, with the checksum field itself
/// zeroed.
///
/// **Not the common CRC32.** zlib's is reflected and inverted; a page
/// checksummed with that one is refused by every other Opus reader while
/// looking perfectly fine to a reader that made the same mistake. Written
/// out rather than taken from a crate for exactly that reason -- it is
/// four lines, and the four lines are the specification.
pub fn page_crc(page: &[u8]) -> u32 {
    let mut crc: u32 = 0;
    for (i, &b) in page.iter().enumerate() {
        // The checksum field reads as nought while it is computed.
        let b = if (22..26).contains(&i) { 0 } else { b };
        crc ^= (b as u32) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 != 0 {
                (crc << 1) ^ 0x04c1_1db7
            } else {
                crc << 1
            };
        }
    }
    crc
}

/// One Ogg page carrying one packet, checksummed.
fn write_page(header_type: u8, granule: u64, serial: u32, seq: u32, packet: &[u8]) -> Vec<u8> {
    let mut segments: Vec<u8> = Vec::new();
    let mut left = packet.len();
    loop {
        let take = left.min(255);
        segments.push(take as u8);
        left -= take;
        if take < 255 {
            break;
        }
        // A packet whose length is a multiple of 255 ends with a
        // nought-length segment, or a reader holds it open for ever.
        if left == 0 {
            segments.push(0);
            break;
        }
    }
    let mut out = Vec::with_capacity(27 + segments.len() + packet.len());
    out.extend_from_slice(b"OggS");
    out.push(0);
    out.push(header_type);
    out.extend_from_slice(&granule.to_le_bytes());
    out.extend_from_slice(&serial.to_le_bytes());
    out.extend_from_slice(&seq.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.push(segments.len() as u8);
    out.extend_from_slice(&segments);
    out.extend_from_slice(packet);
    let crc = page_crc(&out);
    out[22..26].copy_from_slice(&crc.to_le_bytes());
    out
}

/// Wrap Opus packets as an Ogg Opus stream (RFC 7845): `OpusHead`,
/// `OpusTags`, then one page a packet.
///
/// `serial` identifies the stream inside the file. It is arbitrary and it
/// is the caller's, because a caller that wrote two streams with the same
/// one would have written a file no reader can untangle.
pub fn write_ogg(packets: &[Vec<u8>], pre_skip: u16, frame: u32, serial: u32) -> Vec<u8> {
    let mut head = Vec::with_capacity(19);
    head.extend_from_slice(b"OpusHead");
    head.push(1);
    head.push(1); // mono: SIP-18 says a note SHOULD be, and this writes one
    head.extend_from_slice(&pre_skip.to_le_bytes());
    head.extend_from_slice(&RATE.to_le_bytes());
    head.extend_from_slice(&0u16.to_le_bytes()); // output gain
    head.push(0); // channel mapping family 0
    let mut tags = Vec::new();
    tags.extend_from_slice(b"OpusTags");
    // The vendor string. Not decoration: a reader looking at a file that
    // will not play wants to know what wrote it.
    const VENDOR: &str = "sigil";
    tags.extend_from_slice(&(VENDOR.len() as u32).to_le_bytes());
    tags.extend_from_slice(VENDOR.as_bytes());
    tags.extend_from_slice(&0u32.to_le_bytes());

    // The two header packets each take a page of their own, and the first
    // is marked as the beginning of the stream.
    let mut out = write_page(0x02, 0, serial, 0, &head);
    out.extend_from_slice(&write_page(0x00, 0, serial, 1, &tags));
    let mut granule = pre_skip as u64;
    for (seq, (i, packet)) in (2u32..).zip(packets.iter().enumerate()) {
        granule += frame as u64;
        let last = i + 1 == packets.len();
        out.extend_from_slice(&write_page(
            if last { 0x04 } else { 0x00 },
            granule,
            serial,
            seq,
            packet,
        ));
    }
    out
}

/// A decoded note, playing or ready to.
///
/// Position is kept by the device callback -- it counts the samples it
/// actually took -- so the bar under a waveform follows the sound and not
/// a wall clock that drifts from it.
pub struct Note {
    decoded: Arc<Decoded>,
    shared: Arc<Playing>,
    /// Held so the device keeps calling back; dropping it stops the sound.
    _stream: Option<cpal::Stream>,
}

struct Playing {
    /// The next interleaved sample the callback will take, in the note's
    /// own rate and channel count.
    at: AtomicU64,
    playing: AtomicBool,
    ended: AtomicBool,
    ctx: egui::Context,
}

impl Note {
    /// Decode `bytes` and open a device for them.
    ///
    /// **A device that will not open is not a failure to read the note**:
    /// the waveform and the length are already on screen from the message
    /// itself, and they stay there. `played` simply never advances.
    pub fn open(bytes: &[u8], ctx: egui::Context) -> Result<Note, Unreadable> {
        let decoded = Arc::new(decode(bytes)?);
        let shared = Arc::new(Playing {
            at: AtomicU64::new(0),
            playing: AtomicBool::new(false),
            ended: AtomicBool::new(false),
            ctx,
        });
        let stream = open_device(decoded.clone(), shared.clone());
        Ok(Note {
            decoded,
            shared,
            _stream: stream,
        })
    }

    pub fn duration_ms(&self) -> u64 {
        self.decoded.duration_ms()
    }

    /// How far in, in milliseconds.
    pub fn position_ms(&self) -> u64 {
        let frames = self.shared.at.load(Ordering::Relaxed) / self.decoded.channels.max(1) as u64;
        frames * 1000 / RATE as u64
    }

    /// Nought to one, for the mark over the waveform.
    pub fn done(&self) -> f32 {
        let whole = self.decoded.samples.len().max(1) as f32;
        (self.shared.at.load(Ordering::Relaxed) as f32 / whole).clamp(0.0, 1.0)
    }

    pub fn playing(&self) -> bool {
        self.shared.playing.load(Ordering::Relaxed)
    }

    pub fn ended(&self) -> bool {
        self.shared.ended.load(Ordering::Relaxed)
    }

    /// Play, from the start again if it had finished.
    pub fn play(&self) {
        if self.shared.ended.swap(false, Ordering::Relaxed) {
            self.shared.at.store(0, Ordering::Relaxed);
        }
        self.shared.playing.store(true, Ordering::Relaxed);
        self.shared.ctx.request_repaint();
    }

    pub fn pause(&self) {
        self.shared.playing.store(false, Ordering::Relaxed);
        self.shared.ctx.request_repaint();
    }

    pub fn toggle(&self) {
        if self.playing() {
            self.pause();
        } else {
            self.play();
        }
    }

    /// Go to a fraction of the way through, for a press on the waveform.
    pub fn seek(&self, done: f32) {
        let frames = self.decoded.samples.len() / self.decoded.channels.max(1);
        let frame = (done.clamp(0.0, 1.0) * frames as f32) as u64;
        self.shared.at.store(
            frame * self.decoded.channels.max(1) as u64,
            Ordering::Relaxed,
        );
        self.shared.ended.store(false, Ordering::Relaxed);
        self.shared.ctx.request_repaint();
    }
}

/// The output device, filled from the decoded note.
///
/// Linear resampling and channel mapping through [`crate::player::fit`],
/// the same as the video's sound: a device that wants 44.1 kHz stereo gets
/// it from a 48 kHz mono note.
fn open_device(decoded: Arc<Decoded>, shared: Arc<Playing>) -> Option<cpal::Stream> {
    let device = cpal::default_host().default_output_device()?;
    let supported = device.default_output_config().ok()?;
    let config = supported.config();
    let rate = config.sample_rate;
    let channels = config.channels as usize;
    let mut carry: Vec<f32> = Vec::new();
    let stream = device
        .build_output_stream(
            config,
            move |out: &mut [f32], _| {
                if !shared.playing.load(Ordering::Relaxed) {
                    out.fill(0.0);
                    return;
                }
                // How much of the note this callback needs, in its own
                // rate: the device's frames scaled back.
                let want_frames = out.len() / channels.max(1);
                let need = (want_frames as u64 * RATE as u64 / rate.max(1) as u64) as usize + 2;
                let at = shared.at.load(Ordering::Relaxed) as usize;
                let end = (at + need * decoded.channels).min(decoded.samples.len());
                let taken = &decoded.samples[at.min(end)..end];
                let fitted =
                    crate::player::fit(taken, decoded.channels, RATE, channels, rate, &mut carry);
                for (sample, value) in out
                    .iter_mut()
                    .zip(fitted.iter().copied().chain(std::iter::repeat(0.0)))
                {
                    *sample = value;
                }
                shared.at.store(end as u64, Ordering::Relaxed);
                if end >= decoded.samples.len() {
                    shared.playing.store(false, Ordering::Relaxed);
                    shared.ended.store(true, Ordering::Relaxed);
                }
                shared.ctx.request_repaint();
            },
            |_| {},
            None,
        )
        .ok()?;
    stream.play().ok()?;
    Some(stream)
}

/// Recording a voice note: the microphone, Opus, and what it sounds like
/// so far.
///
/// # What it is and is not
///
/// SIP-18 again: a note is **a file, not a stream**. Nothing here is sent
/// as it is spoken, nothing is timestamped, and a dropped frame is a
/// recording that is short rather than a gap somebody hears. So this
/// collects, and `finish` hands over the whole thing at once.
///
/// **24 kbit/s mono, the rate the spec recommends** -- "the rate the
/// reference voice implementation settled on". Six seconds is about 18 kB,
/// which is a thumbnail.
pub struct Recorder {
    shared: Arc<Capture>,
    /// The thread that owns the stream. It opens the device, keeps it
    /// alive, and drops it when `stop` is set -- which is how a `cpal`
    /// stream that must not cross threads is held without holding the
    /// interface up.
    thread: Option<std::thread::JoinHandle<()>>,
}

/// What the microphone's callback fills and the interface reads.
struct Capture {
    /// Mono samples at [`RATE`], waiting to be encoded.
    pending: std::sync::Mutex<Vec<f32>>,
    /// How much has been taken in, in samples, so the length is the
    /// device's count and not a wall clock that drifts from it.
    taken: AtomicU64,
    /// The loudest recent moment, on SIP-15's scale, for a meter that
    /// shows somebody the microphone is hearing them.
    level: std::sync::atomic::AtomicU8,
    /// The device would not open. Said rather than recorded in silence.
    deaf: AtomicBool,
    /// Set by `finish`, read by the thread that holds the stream.
    stop: AtomicBool,
    ctx: egui::Context,
}

/// One Opus frame: 20 ms at 48 kHz, which is what a call uses and what
/// every Opus encoder is happiest with.
pub const FRAME: usize = 960;

impl Recorder {
    /// Start taking sound in.
    ///
    /// **Returns at once.** Opening an input device is not quick -- it
    /// took thirty-six seconds on a Mac being asked for permission, and on
    /// a phone it is long enough to see -- so the device is opened on a
    /// thread of its own and this returns a recorder that has heard
    /// nothing yet. A press that froze the interface until the microphone
    /// was ready would be a press nobody makes twice.
    ///
    /// Never fails: a device that will not open is a recorder that hears
    /// nothing and says so ([`Recorder::deaf`]), because the alternative
    /// is a button that does nothing on the one phone whose microphone is
    /// busy with a call.
    pub fn start(ctx: egui::Context) -> Recorder {
        let shared = Arc::new(Capture {
            pending: std::sync::Mutex::new(Vec::new()),
            taken: AtomicU64::new(0),
            level: std::sync::atomic::AtomicU8::new(255),
            deaf: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            ctx,
        });
        let mine = shared.clone();
        let thread = std::thread::Builder::new()
            .name("sigil-voice-note".into())
            .spawn(move || {
                let stream = open_microphone(mine.clone());
                if stream.is_none() {
                    mine.deaf.store(true, Ordering::Relaxed);
                    mine.ctx.request_repaint();
                    return;
                }
                while !mine.stop.load(Ordering::Relaxed) {
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                // Dropped here, on the thread that made it: some backends
                // require that, and all of them stop the microphone.
                drop(stream);
            })
            .ok();
        Recorder { shared, thread }
    }

    /// The microphone would not open, so there will be nothing to send.
    pub fn deaf(&self) -> bool {
        self.shared.deaf.load(Ordering::Relaxed)
    }

    /// How long it has been recording.
    pub fn elapsed_ms(&self) -> u64 {
        self.shared.taken.load(Ordering::Relaxed) * 1000 / RATE as u64
    }

    /// The loudest recent moment on SIP-15's scale: nought is full scale,
    /// 255 is digital silence. The same number a bar in a waveform is.
    pub fn level(&self) -> u8 {
        self.shared.level.load(Ordering::Relaxed)
    }

    /// Stop, encode, and hand over the file.
    ///
    /// `None` when nothing was heard -- a microphone that would not open,
    /// or a press so short that not one 20 ms frame went by. A caller that
    /// sent that would be sending an empty file with a length of nought.
    pub fn finish(mut self) -> Option<Vec<u8>> {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let samples = std::mem::take(&mut *self.shared.pending.lock().unwrap());
        if samples.len() < FRAME {
            return None;
        }
        encode(&samples)
    }
}

impl Drop for Recorder {
    /// Thrown away: the thread is told to stop and the microphone goes
    /// with it. Not joined -- a recording somebody discarded must not hold
    /// the interface up while a device closes.
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
    }
}

/// Mono samples at [`RATE`] as an Ogg Opus file, at the rate SIP-18
/// recommends.
pub fn encode(samples: &[f32]) -> Option<Vec<u8>> {
    let mut encoder =
        opus::Encoder::new(RATE, opus::Channels::Mono, opus::Application::Voip).ok()?;
    // Not a guess: "RECOMMENDED 24 kbit/s -- the rate the reference voice
    // implementation settled on."
    let _ = encoder.set_bitrate(opus::Bitrate::Bits(24_000));
    let mut packets: Vec<Vec<u8>> = Vec::new();
    for frame in samples.chunks(FRAME) {
        // The last part-frame is padded rather than dropped: a note that
        // ends mid-word should end mid-word, not a fifth of a second
        // earlier.
        let padded;
        let frame = if frame.len() == FRAME {
            frame
        } else {
            padded = {
                let mut v = frame.to_vec();
                v.resize(FRAME, 0.0);
                v
            };
            &padded
        };
        match encoder.encode_vec_float(frame, 4000) {
            Ok(packet) => packets.push(packet),
            Err(_) => break,
        }
    }
    if packets.is_empty() {
        return None;
    }
    // A serial that is this recording's and nothing else's. The clock is
    // enough: two notes from one phone are not made in the same
    // nanosecond, and nothing else reads it.
    let serial = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() ^ d.as_secs() as u32)
        .unwrap_or(1);
    Some(write_ogg(&packets, 0, FRAME as u32, serial))
}

/// The microphone, resampled to what Opus wants.
fn open_microphone(shared: Arc<Capture>) -> Option<cpal::Stream> {
    let device = cpal::default_host().default_input_device()?;
    let supported = device.default_input_config().ok()?;
    let config = supported.config();
    let rate = config.sample_rate;
    let channels = config.channels as usize;
    // **Said, because a recording that comes out at half speed looks
    // exactly like one that came out right until somebody listens to it.**
    // The resampling is built on what the device claims, and nothing else
    // in the file records what that was.
    tracing::info!(rate, channels, "voice note: the microphone");
    let mut carry: Vec<f32> = Vec::new();
    let stream = device
        .build_input_stream(
            config,
            move |input: &[f32], _| {
                // Down to mono at 48 kHz, through the same `fit` the
                // player uses in the other direction.
                let mono = crate::player::fit(input, channels, rate, 1, RATE, &mut carry);
                if mono.is_empty() {
                    return;
                }
                let loud = (mono.iter().map(|s| s * s).sum::<f32>() / mono.len() as f32).sqrt();
                let level = if loud <= 0.0 {
                    255
                } else {
                    (-2.0 * 20.0 * loud.log10()).round().clamp(0.0, 254.0) as u8
                };
                // The loudest of the last moment rather than this
                // callback's: a meter that fell to silence between
                // syllables would read as a microphone that had stopped.
                let was = shared.level.load(Ordering::Relaxed);
                shared
                    .level
                    .store(level.min(was.saturating_add(4)), Ordering::Relaxed);
                shared.taken.fetch_add(mono.len() as u64, Ordering::Relaxed);
                shared.pending.lock().unwrap().extend_from_slice(&mono);
                shared.ctx.request_repaint();
            },
            |_| {},
            None,
        )
        .ok()?;
    stream.play().ok()?;
    Some(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An Ogg page, as RFC 3533 lays one out. Used to build the fixtures:
    /// the parser is read against bytes assembled by this and not by
    /// itself, so a misreading of the layout would have to be made twice
    /// and identically to pass.
    fn page_bytes(header_type: u8, granule: u64, seq: u32, packets: &[&[u8]]) -> Vec<u8> {
        let mut segments: Vec<u8> = Vec::new();
        let mut body: Vec<u8> = Vec::new();
        for p in packets {
            let mut left = p.len();
            let mut at = 0;
            loop {
                let take = left.min(255);
                segments.push(take as u8);
                body.extend_from_slice(&p[at..at + take]);
                at += take;
                left -= take;
                // A packet whose length is a multiple of 255 ends with a
                // nought-length segment, or the reader would hold it open.
                if take < 255 {
                    break;
                }
                if left == 0 {
                    segments.push(0);
                    break;
                }
            }
        }
        let mut out = Vec::new();
        out.extend_from_slice(b"OggS");
        out.push(0);
        out.push(header_type);
        out.extend_from_slice(&granule.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&seq.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.push(segments.len() as u8);
        out.extend_from_slice(&segments);
        out.extend_from_slice(&body);
        out
    }

    fn opus_head(channels: u8, pre_skip: u16) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"OpusHead");
        out.push(1);
        out.push(channels);
        out.extend_from_slice(&pre_skip.to_le_bytes());
        out.extend_from_slice(&48_000u32.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.push(0);
        out
    }

    fn opus_tags() -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"OpusTags");
        out.extend_from_slice(&4u32.to_le_bytes());
        out.extend_from_slice(b"none");
        out.extend_from_slice(&0u32.to_le_bytes());
        out
    }

    /// What this writes, this reads: the round trip is the first thing a
    /// writer has to earn.
    #[test]
    fn what_is_written_decodes_again() {
        let ogg = write_ogg(&tone_packets(500), 312, FRAME as u32, 7);
        let decoded = decode(&ogg).expect("decodes");
        assert_eq!(decoded.channels, 1);
        let ms = decoded.duration_ms();
        assert!((480..=500).contains(&ms), "{ms} ms");
    }

    /// **Every page carries a correct Ogg CRC.** Checked by recomputing
    /// it over each page, which is the check every other Opus reader
    /// makes -- sigil's own reader does not, so nothing else in this
    /// crate would notice a writer that got it wrong.
    #[test]
    fn every_page_is_checksummed() {
        let ogg = write_ogg(&tone_packets(500), 0, FRAME as u32, 7);
        let mut at = 0;
        let mut pages = 0;
        while let Some(p) = page(&ogg, at) {
            let whole = &ogg[at..p.next];
            let carried = u32::from_le_bytes(whole[22..26].try_into().unwrap());
            assert_eq!(page_crc(whole), carried, "page {pages} at {at}");
            assert_ne!(carried, 0, "page {pages} carried no checksum at all");
            pages += 1;
            at = p.next;
        }
        assert!(pages > 3, "only {pages} pages");
    }

    /// The negative control for the one above: a byte changed anywhere in
    /// a page makes its checksum wrong. Without this, a `page_crc` that
    /// returned a constant would satisfy the test above.
    #[test]
    fn a_changed_byte_breaks_the_checksum() {
        let mut ogg = write_ogg(&tone_packets(200), 0, FRAME as u32, 7);
        let first = page(&ogg, 0).expect("a page");
        let whole = first.next;
        let was = page_crc(&ogg[..whole]);
        // A byte in the body, past the header.
        ogg[30] ^= 0xff;
        assert_ne!(page_crc(&ogg[..whole]), was);
    }

    /// Ogg's CRC is not zlib's. A writer that reached for the usual one
    /// produces a file every other reader refuses and this crate's own
    /// reader accepts, because it does not check.
    ///
    /// The vector is RFC 3533's polynomial over `b"sigil"`, computed the
    /// long way here; zlib's over the same bytes is a different number,
    /// which is the whole point of pinning it.
    #[test]
    fn the_checksum_is_oggs_and_not_zlibs() {
        // A page short enough that the checksum field is past its end, so
        // `page_crc` zeroes nothing and this is the bare polynomial.
        let plain = page_crc(b"sigil");
        let mut by_hand: u32 = 0;
        for &b in b"sigil" {
            by_hand ^= (b as u32) << 24;
            for _ in 0..8 {
                by_hand = if by_hand & 0x8000_0000 != 0 {
                    (by_hand << 1) ^ 0x04c1_1db7
                } else {
                    by_hand << 1
                };
            }
        }
        assert_eq!(plain, by_hand);
        // zlib's, for the same bytes: reflected, inverted, 0xedb88320.
        let mut zlib: u32 = 0xffff_ffff;
        for &b in b"sigil" {
            zlib ^= b as u32;
            for _ in 0..8 {
                zlib = if zlib & 1 != 0 {
                    (zlib >> 1) ^ 0xedb8_8320
                } else {
                    zlib >> 1
                };
            }
        }
        assert_ne!(
            plain,
            zlib ^ 0xffff_ffff,
            "the two CRCs agreed, so one is wrong"
        );
    }

    /// The last page says the stream ended there, and no other page does.
    /// A reader that trusts the flag -- most do -- stops at the first page
    /// wrongly marked.
    #[test]
    fn only_the_last_page_ends_the_stream() {
        let ogg = write_ogg(&tone_packets(500), 0, FRAME as u32, 7);
        let mut at = 0;
        let mut ends = Vec::new();
        let mut seen = 0;
        while let Some(p) = page(&ogg, at) {
            if p.header_type & 0x04 != 0 {
                ends.push(seen);
            }
            seen += 1;
            at = p.next;
        }
        assert_eq!(ends, vec![seen - 1], "{seen} pages, ends at {ends:?}");
    }

    /// What the recorder encodes is a file this reads back at the length
    /// it was given: the round trip a recording actually makes, with only
    /// the microphone left out.
    #[test]
    fn what_is_recorded_encodes_to_a_note_of_the_same_length() {
        let samples: Vec<f32> = (0..RATE as usize)
            .map(|i| {
                let t = i as f32 / RATE as f32;
                (t * 300.0 * std::f32::consts::TAU).sin() * 0.4
            })
            .collect();
        let ogg = encode(&samples).expect("encodes");
        let decoded = decode(&ogg).expect("decodes");
        let ms = decoded.duration_ms();
        assert!((980..=1020).contains(&ms), "a second came back as {ms} ms");
        assert_eq!(decoded.channels, 1, "SIP-18 says mono");
    }

    /// A press and an immediate release is not a voice note.
    #[test]
    fn a_recording_shorter_than_a_frame_is_nothing() {
        assert!(encode(&[]).is_none());
        assert!(
            encode(&vec![0.1; FRAME / 2]).is_some(),
            "half a frame is padded, not dropped"
        );
    }

    /// The bitrate SIP-18 recommends, checked by its consequence: a
    /// second of speech at 24 kbit/s is about three kilobytes, and
    /// nothing near the 4000-byte-a-packet ceiling the encoder is given.
    #[test]
    fn a_note_is_encoded_at_about_the_recommended_rate() {
        let samples: Vec<f32> = (0..RATE as usize)
            .map(|i| {
                let t = i as f32 / RATE as f32;
                (t * 300.0 * std::f32::consts::TAU).sin() * 0.4
            })
            .collect();
        let ogg = encode(&samples).expect("encodes");
        // Ogg's own framing is about 30 bytes a page and there are 50.
        let audio = ogg.len() as f32 - 1_800.0;
        let kbit = audio * 8.0 / 1000.0;
        assert!(
            (12.0..40.0).contains(&kbit),
            "a second came out at {kbit:.1} kbit/s"
        );
    }

    fn tone_packets(ms: usize) -> Vec<Vec<u8>> {
        let mut encoder =
            opus::Encoder::new(RATE, opus::Channels::Mono, opus::Application::Voip).unwrap();
        (0..ms / 20)
            .map(|f| {
                let pcm: Vec<f32> = (0..FRAME)
                    .map(|i| {
                        let t = (f * FRAME + i) as f32 / RATE as f32;
                        (t * 440.0 * std::f32::consts::TAU).sin() * 0.5
                    })
                    .collect();
                encoder.encode_vec_float(&pcm, 4000).unwrap()
            })
            .collect()
    }

    /// A real note: a tone, encoded by libopus, wrapped in Ogg.
    ///
    /// Encoded rather than hand-written because what is being tested is
    /// that the *decoder* gets whole, correct packets -- a fixture of made
    /// up bytes would prove only that the parser does not crash.
    fn a_note(ms: usize, pre_skip: u16) -> Vec<u8> {
        const FRAME: usize = 960; // 20 ms at 48 kHz
        let mut encoder =
            opus::Encoder::new(RATE, opus::Channels::Mono, opus::Application::Voip).unwrap();
        let mut packets: Vec<Vec<u8>> = Vec::new();
        let frames = ms / 20;
        for f in 0..frames {
            let pcm: Vec<f32> = (0..FRAME)
                .map(|i| {
                    let t = (f * FRAME + i) as f32 / RATE as f32;
                    (t * 440.0 * std::f32::consts::TAU).sin() * 0.5
                })
                .collect();
            packets.push(encoder.encode_vec_float(&pcm, 4000).unwrap());
        }
        let mut out = page_bytes(0x02, 0, 0, &[&opus_head(1, pre_skip)]);
        out.extend_from_slice(&page_bytes(0x00, 0, 1, &[&opus_tags()]));
        let mut granule = pre_skip as u64;
        for (seq, (i, packet)) in (2u32..).zip(packets.iter().enumerate()) {
            granule += FRAME as u64;
            let last = i + 1 == packets.len();
            out.extend_from_slice(&page_bytes(
                if last { 0x04 } else { 0x00 },
                granule,
                seq,
                &[packet],
            ));
        }
        out
    }

    #[test]
    fn a_voice_note_decodes_to_the_length_it_claims() {
        let note = a_note(1000, 312);
        let decoded = decode(&note).expect("decodes");
        assert_eq!(decoded.channels, 1);
        // A second, less the pre-skip the encoder asked for.
        let ms = decoded.duration_ms();
        assert!((990..=1000).contains(&ms), "{ms} ms");
    }

    /// The pre-skip is not decoration: RFC 7845 has the encoder put
    /// samples in front and the player throw them away. Without that a
    /// note begins with a click and runs long.
    #[test]
    fn the_pre_skip_is_dropped() {
        let with = decode(&a_note(1000, 0)).expect("decodes");
        let skipped = decode(&a_note(1000, 960)).expect("decodes");
        assert_eq!(
            with.samples.len() - skipped.samples.len(),
            960,
            "the pre-skip was not taken off the front"
        );
    }

    /// A packet longer than one segment spans them, and the reader has to
    /// put it back together: a 255-byte boundary in the middle of an Opus
    /// packet is the one thing an Ogg reader gets wrong.
    #[test]
    fn a_packet_spanning_segments_is_rejoined() {
        let long: Vec<u8> = (0..600u32).map(|i| (i % 251) as u8).collect();
        let ogg = page_bytes(0x02, 0, 0, &[&long]);
        let (packets, _) = packets(&ogg).expect("reads");
        assert_eq!(packets.len(), 1, "the segments were not rejoined");
        assert_eq!(packets[0], long);
    }

    /// The negative controls. Each of these is a file somebody could hand
    /// this, and none of them may be treated as a note.
    #[test]
    fn what_is_not_a_voice_note_is_refused() {
        assert_eq!(decode(b"").unwrap_err(), Unreadable::NotAVoiceNote);
        assert_eq!(
            decode(b"not an ogg file at all").unwrap_err(),
            Unreadable::NotAVoiceNote
        );
        // Ogg, and carrying something that is not Opus.
        let vorbis = page_bytes(0x02, 0, 0, &[b"\x01vorbis........."]);
        assert_eq!(decode(&vorbis).unwrap_err(), Unreadable::NotAVoiceNote);
        // Opus, and truncated in the middle of a page -- which is what a
        // blob whose last chunk never arrived looks like. The half page is
        // dropped and what did arrive plays: a note cut short is better
        // than no note, and it is the reader stopping at a page it cannot
        // read whole that makes that safe.
        let note = a_note(200, 0);
        let cut = &note[..note.len() - 40];
        let short = decode(cut).expect("what arrived decodes");
        assert!(
            short.duration_ms() < decode(&note).unwrap().duration_ms(),
            "the truncated note decoded to the whole length"
        );
    }

    /// A file claiming hours is refused before a byte of it is decoded --
    /// the length is read off the last page's granule, which costs nothing
    /// and cannot be made to allocate.
    #[test]
    fn a_note_claiming_to_be_hours_long_is_refused_before_decoding() {
        let mut note = a_note(200, 0);
        // Rewrite the last page's granule to a day at 48 kHz.
        let day = 48_000u64 * 60 * 60 * 24;
        let mut at = 0;
        let mut last = 0;
        while let Some(p) = page(&note, at) {
            last = at;
            at = p.next;
        }
        note[last + 6..last + 14].copy_from_slice(&day.to_le_bytes());
        assert_eq!(decode(&note).unwrap_err(), Unreadable::TooLong);
    }
}
