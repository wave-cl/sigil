//! H.264 pictures out of a demuxed stream.

use openh264::decoder::{Decoder, DecoderConfig, Flush};
use openh264::formats::YUVSource;

/// A decoder that never flushes after a decode.
///
/// **The crate's default flushes, and flushing breaks this stream.** With
/// it, the tenth picture of an ordinary 720p High-profile file came back
/// as "out of memory" and every picture after it as "no parameter sets";
/// without it, all of them decode. The crate's own comment on flushing says
/// its best practice is unknown, so this takes the mode that works and
/// reads pictures out as they come.
fn fresh_decoder() -> Result<Decoder, openh264::Error> {
    Decoder::with_api_config(
        openh264::OpenH264API::from_source(),
        DecoderConfig::new().flush_after_decode(Flush::NoFlush),
    )
}

use crate::demux::{Demuxer, Sample};

/// One decoded picture.
pub struct Frame {
    pub at_ms: u64,
    pub image: egui::ColorImage,
}

pub struct Pictures {
    demuxer: Demuxer,
    decoder: Decoder,
    /// Next sample id to decode, 1-based, decode order.
    next: u32,
    /// Presentation times of the pictures still to come out, in order.
    ///
    /// **The decoder reorders.** openh264 hands pictures back in display
    /// order, a sample or two behind what was fed, and says nothing about
    /// which sample a picture was. The first version labelled each output
    /// with the time of the sample just fed -- decode order -- and then
    /// sorted by those labels, and what reached the screen was a permutation
    /// of neighbouring pictures: on time, every one, and the wrong one, three
    /// times in four, which looks exactly like judder. Compared against
    /// ffmpeg's decode frame by frame, output *k* is display frame *k*; so
    /// the *k*-th picture out gets the *k*-th presentation time from the
    /// keyframe decoding started at.
    coming: std::collections::VecDeque<u64>,
    /// Pictures the decoder gave up at the end of the stream, in order.
    flushed: std::collections::VecDeque<Frame>,
    pub skipped_empty: u32,
    pub skipped_err: u32,
    pub last_err: Option<String>,
}

impl Pictures {
    pub fn new(demuxer: Demuxer) -> Result<Pictures, String> {
        let decoder = fresh_decoder().map_err(|e| format!("H.264 decoder: {e}"))?;
        let coming = demuxer.presentation_from(1).collect();
        Ok(Pictures {
            demuxer,
            decoder,
            next: 1,
            coming,
            flushed: std::collections::VecDeque::new(),
            skipped_empty: 0,
            skipped_err: 0,
            last_err: None,
        })
    }

    pub fn demuxer(&self) -> &Demuxer {
        &self.demuxer
    }

    /// Start decoding from the keyframe at or before `ms`. Pictures before
    /// `ms` still come out (the decoder needs them) and are handed back;
    /// the caller drops what is earlier than it wants.
    pub fn seek(&mut self, ms: u64) {
        self.next = self.demuxer.keyframe_before(ms);
        self.coming = self.demuxer.presentation_from(self.next).collect();
        self.flushed.clear();
        // A fresh decoder: what it held was from another place in the
        // stream, and the keyframe carries everything it needs.
        if let Ok(d) = fresh_decoder() {
            self.decoder = d;
        }
    }

    /// The next picture in display order, or `None` at the end.
    ///
    /// A sample that yields no picture yet (the decoder is holding it) is
    /// followed by the next until one comes out.
    pub fn decode_next(&mut self) -> Option<Frame> {
        loop {
            if let Some(f) = self.flushed.pop_front() {
                return Some(f);
            }
            let Some(Sample { annex_b, .. }) = self.demuxer.sample(self.next) else {
                // The end of the stream. The decoder is still holding the
                // last picture or two; without this they are never shown
                // and a two-second clip is fifty-nine pictures long.
                if !self.coming.is_empty() {
                    let rest = self.decoder.flush_remaining().unwrap_or_default();
                    let mut out = Vec::new();
                    for yuv in &rest {
                        let (w, h) = yuv.dimensions();
                        let mut rgba = vec![0u8; w * h * 4];
                        yuv.write_rgba8(&mut rgba);
                        out.push(egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba));
                    }
                    drop(rest);
                    for image in out {
                        let Some(at_ms) = self.coming.pop_front() else {
                            break;
                        };
                        self.flushed.push_back(Frame { at_ms, image });
                    }
                }
                self.coming.clear();
                return self.flushed.pop_front();
            };
            self.next += 1;
            match self.decoder.decode(&annex_b) {
                Ok(Some(yuv)) => {
                    let (w, h) = yuv.dimensions();
                    let mut rgba = vec![0u8; w * h * 4];
                    yuv.write_rgba8(&mut rgba);
                    let image = egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba);
                    let at_ms = self.coming.pop_front()?;
                    return Some(Frame { at_ms, image });
                }
                Ok(None) => {
                    self.skipped_empty += 1;
                    continue;
                }
                Err(e) => {
                    self.skipped_err += 1;
                    self.last_err = Some(e.to_string());
                    continue;
                }
            }
        }
    }
}

/// Pictures as the decoder hands them out, which is display order.
///
/// Kept as a name because callers had one: the reordering it did was
/// undoing the decoder's own, and is gone. See [`Pictures::coming`].
pub struct Ordered {
    pictures: Pictures,
}

impl Ordered {
    pub fn new(pictures: Pictures) -> Ordered {
        Ordered { pictures }
    }

    pub fn demuxer(&self) -> &Demuxer {
        self.pictures.demuxer()
    }

    pub fn pictures(&self) -> &Pictures {
        &self.pictures
    }

    pub fn seek(&mut self, ms: u64) {
        self.pictures.seek(ms);
    }

    pub fn decode_next(&mut self) -> Option<Frame> {
        self.pictures.decode_next()
    }
}
