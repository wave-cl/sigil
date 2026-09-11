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
    /// Composition times of samples fed and not yet answered with a
    /// picture, oldest first. The decoder answers a sample with a picture
    /// one or two samples later, in the order fed, and does not say which;
    /// this does.
    fed: std::collections::VecDeque<u64>,
    /// Pictures the decoder gave up at the end of the stream, in order.
    flushed: std::collections::VecDeque<Frame>,
    rgba: Vec<u8>,
    pub skipped_empty: u32,
    pub skipped_err: u32,
    pub last_err: Option<String>,
}

impl Pictures {
    pub fn new(demuxer: Demuxer) -> Result<Pictures, String> {
        let decoder = fresh_decoder().map_err(|e| format!("H.264 decoder: {e}"))?;
        Ok(Pictures {
            demuxer,
            decoder,
            next: 1,
            fed: std::collections::VecDeque::new(),
            flushed: std::collections::VecDeque::new(),
            rgba: Vec::new(),
            skipped_empty: 0,
            skipped_err: 0,
            last_err: None,
        })
    }

    pub fn demuxer(&self) -> &Demuxer {
        &self.demuxer
    }

    /// Start decoding from the keyframe at or before `ms`. Pictures before
    /// `ms` are still decoded (the decoder needs them) and handed back;
    /// the caller drops what is earlier than it wants.
    pub fn seek(&mut self, ms: u64) {
        self.next = self.demuxer.keyframe_before(ms);
        self.fed.clear();
        self.flushed.clear();
        // A fresh decoder: what it held was from another place in the
        // stream, and the keyframe carries everything it needs.
        if let Ok(d) = fresh_decoder() {
            self.decoder = d;
        }
    }

    /// The next picture in decode order, or `None` at the end.
    ///
    /// A sample that yields no picture yet (the decoder is holding it for
    /// reordering) is followed by the next until one comes out.
    pub fn decode_next(&mut self) -> Option<Frame> {
        loop {
            if let Some(f) = self.flushed.pop_front() {
                return Some(f);
            }
            let Some(Sample { at_ms, annex_b, .. }) = self.demuxer.sample(self.next) else {
                // The end of the stream. The decoder is still holding the
                // last picture or two; without this they are never shown
                // and a two-second clip is fifty-nine pictures long.
                if !self.fed.is_empty()
                    && let Ok(rest) = self.decoder.flush_remaining()
                {
                    for yuv in rest {
                        let (w, h) = yuv.dimensions();
                        self.rgba.resize(w * h * 4, 0);
                        yuv.write_rgba8(&mut self.rgba);
                        let image = egui::ColorImage::from_rgba_unmultiplied([w, h], &self.rgba);
                        let Some(at_ms) = self.fed.pop_front() else {
                            break;
                        };
                        self.flushed.push_back(Frame { at_ms, image });
                    }
                }
                self.fed.clear();
                return self.flushed.pop_front();
            };
            self.next += 1;
            self.fed.push_back(at_ms);
            match self.decoder.decode(&annex_b) {
                Ok(Some(yuv)) => {
                    let (w, h) = yuv.dimensions();
                    self.rgba.resize(w * h * 4, 0);
                    yuv.write_rgba8(&mut self.rgba);
                    let image = egui::ColorImage::from_rgba_unmultiplied([w, h], &self.rgba);
                    // Pictures come out in the order fed, a sample or two
                    // behind, so this one is the oldest still owed. The
                    // caller orders them for display, since decode order is
                    // not display order when there are B-frames: see
                    // `Ordered`.
                    let at_ms = self.fed.pop_front().unwrap_or(at_ms);
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

/// Pictures in the order they are shown.
///
/// H.264 stores a B-frame after the pictures it is predicted from, which are
/// shown after it. Decoded with no delay, pictures come out in that stored
/// order; this holds a few back and hands out the earliest, which is the
/// reordering a decoder with a delay would have done itself.
pub struct Ordered {
    pictures: Pictures,
    held: Vec<Frame>,
    done: bool,
}

/// How many pictures are held back for reordering. H.264 allows more, but
/// what phones and encoders produce is two or three B-frames between
/// references, and each held picture is a frame of latency at a seek.
const REORDER: usize = 4;

impl Ordered {
    pub fn new(pictures: Pictures) -> Ordered {
        Ordered {
            pictures,
            held: Vec::new(),
            done: false,
        }
    }

    pub fn demuxer(&self) -> &Demuxer {
        self.pictures.demuxer()
    }

    pub fn pictures(&self) -> &Pictures {
        &self.pictures
    }

    pub fn seek(&mut self, ms: u64) {
        self.pictures.seek(ms);
        self.held.clear();
        self.done = false;
    }

    /// The next picture in display order.
    pub fn decode_next(&mut self) -> Option<Frame> {
        while !self.done && self.held.len() < REORDER {
            match self.pictures.decode_next() {
                Some(f) => self.held.push(f),
                None => self.done = true,
            }
        }
        if self.held.is_empty() {
            return None;
        }
        let (i, _) = self.held.iter().enumerate().min_by_key(|(_, f)| f.at_ms)?;
        Some(self.held.swap_remove(i))
    }
}
