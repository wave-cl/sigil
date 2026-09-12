//! The sound of a file: AAC out of the MP4, as interleaved f32.
//!
//! symphonia reads the container itself, so the audio side never touches
//! the video demuxer: two readers over one `Arc<[u8]>`, each seeing only
//! its own track.

use std::io::Cursor;

use symphonia::core::audio::GenericAudioBufferRef;
use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::{AudioDecoder, AudioDecoderOptions};
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::{MediaSourceStream, MediaSourceStreamOptions};
use symphonia::core::meta::MetadataOptions;
use symphonia::core::units::Time;

/// A run of decoded sound.
pub struct Chunk {
    /// Where in the file it starts, milliseconds.
    pub at_ms: u64,
    /// Interleaved, `channels` wide.
    pub samples: Vec<f32>,
}

pub struct Sound {
    format: Box<dyn FormatReader + 'static>,
    decoder: Box<dyn AudioDecoder>,
    /// The file, kept so the reader can be made again: once it has read to
    /// the end it will not seek back, and "play again" is a seek to nought.
    bytes: std::sync::Arc<[u8]>,
    track: u32,
    time_base: Option<symphonia::core::units::TimeBase>,
    pub rate: u32,
    pub channels: usize,
}

impl Sound {
    /// `None` when the file has no sound this can decode; a video without
    /// a soundtrack plays silent rather than not at all.
    pub fn open(bytes: std::sync::Arc<[u8]>) -> Option<Sound> {
        let mss = MediaSourceStream::new(
            Box::new(Cursor::new(bytes.clone())),
            MediaSourceStreamOptions::default(),
        );
        let mut hint = Hint::new();
        hint.with_extension("mp4");
        let format = symphonia::default::get_probe()
            .probe(
                &hint,
                mss,
                FormatOptions::default(),
                MetadataOptions::default(),
            )
            .ok()?;
        let track = format.default_track(TrackType::Audio)?;
        let Some(CodecParameters::Audio(params)) = &track.codec_params else {
            return None;
        };
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(params, &AudioDecoderOptions::default())
            .ok()?;
        let rate = params.sample_rate?;
        let channels = params.channels.as_ref()?.count();
        Some(Sound {
            track: track.id,
            time_base: track.time_base,
            format,
            decoder,
            bytes,
            rate,
            channels,
        })
    }

    /// Continue from `ms`.
    ///
    /// **On a fresh reader every time.** One that has reached the end
    /// accepts a seek and then hands back nothing (symphonia's MP4 reader,
    /// at least), which made playing a clip a second time impossible. Opening
    /// is under a millisecond, so the reader is simply made again over the
    /// same bytes and sought from there.
    pub fn seek(&mut self, ms: u64) {
        if let Some(fresh) = Sound::open(self.bytes.clone()) {
            self.format = fresh.format;
            self.decoder = fresh.decoder;
        }
        let _ = self.format.seek(
            SeekMode::Accurate,
            SeekTo::Time {
                time: Time::from_millis_u64(ms),
                track_id: Some(self.track),
            },
        );
        self.decoder.reset();
    }

    /// The next run of sound, or `None` at the end.
    pub fn decode_next(&mut self) -> Option<Chunk> {
        loop {
            let packet = self.format.next_packet().ok()??;
            if packet.track_id != self.track {
                continue;
            }
            let at_ms = self
                .time_base
                .and_then(|tb| tb.calc_time(packet.pts))
                .map(|t| (t.as_secs_f64() * 1000.0) as u64)
                .unwrap_or(0);
            let Ok(decoded) = self.decoder.decode(&packet) else {
                continue;
            };
            let mut samples = Vec::new();
            copy(&decoded, &mut samples);
            return Some(Chunk { at_ms, samples });
        }
    }
}

fn copy(buf: &GenericAudioBufferRef<'_>, out: &mut Vec<f32>) {
    buf.copy_to_vec_interleaved(out);
}
