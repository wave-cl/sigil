//! Take an MP4 apart: what is in it, and the H.264 samples in order with
//! what the decoder needs to make sense of them.
//!
//! The container is read once into a sample table; each sample is then cut
//! out of the bytes on demand. An MP4 stores a picture's NAL units with a
//! length in front of each; a decoder wants Annex B, which is a start code
//! in front of each, and the parameter sets (SPS, PPS) in the stream before
//! the first picture rather than in a box off to the side. `sample` does
//! that translation, so the decoder never sees the container.

use std::io::Cursor;

use mp4::{MediaType, Mp4Reader};

/// What was found in the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Description {
    pub width: u32,
    pub height: u32,
    /// Whole file, in milliseconds.
    pub duration_ms: u64,
    pub frames: u32,
    pub has_audio: bool,
}

/// One picture's compressed bytes, ready for the decoder.
pub struct Sample {
    /// Presentation time, milliseconds from the start.
    pub at_ms: u64,
    /// A keyframe: decoding can start here.
    pub sync: bool,
    /// Annex B: start codes, parameter sets ahead of a keyframe.
    pub annex_b: Vec<u8>,
}

pub struct Demuxer {
    reader: Mp4Reader<Cursor<std::sync::Arc<[u8]>>>,
    track: u32,
    timescale: u32,
    length_size: usize,
    /// SPS then PPS, each with a start code, put ahead of every keyframe.
    parameter_sets: Vec<u8>,
    frames: u32,
    /// Presentation order: sample ids sorted by composition time, with the
    /// time of each. H.264 may store pictures out of the order they are
    /// shown in (B-frames), and `read_sample` walks decode order.
    order: Vec<(u64, u32)>,
    /// The earliest composition time, taken off every time: an encoder
    /// that reorders puts the first picture a frame or two after zero and
    /// writes an edit list to hide it, and this is what the edit list
    /// would do.
    first_ms: u64,
    description: Description,
}

/// Why a file will not play.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unplayable {
    /// Not an MP4 at all, or one this reader cannot parse.
    NotMp4(String),
    /// An MP4 with no video track, or one whose codec is not H.264.
    Codec(String),
}

impl std::fmt::Display for Unplayable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unplayable::NotMp4(e) => write!(f, "not an MP4 this can read: {e}"),
            Unplayable::Codec(what) => write!(f, "{what}"),
        }
    }
}

impl Demuxer {
    pub fn open(bytes: std::sync::Arc<[u8]>) -> Result<Demuxer, Unplayable> {
        let size = bytes.len() as u64;
        let reader = Mp4Reader::read_header(Cursor::new(bytes), size)
            .map_err(|e| Unplayable::NotMp4(e.to_string()))?;
        let video = reader
            .tracks()
            .values()
            .find(|t| t.track_type().ok() == Some(mp4::TrackType::Video))
            .ok_or_else(|| Unplayable::Codec("no video track".into()))?;
        match video.media_type() {
            Ok(MediaType::H264) => {}
            Ok(other) => {
                return Err(Unplayable::Codec(format!(
                    "{other} video is not something sigil plays yet (H.264 is)"
                )));
            }
            Err(e) => return Err(Unplayable::Codec(e.to_string())),
        }
        let avcc = video
            .trak
            .mdia
            .minf
            .stbl
            .stsd
            .avc1
            .as_ref()
            .map(|a| &a.avcc)
            .ok_or_else(|| Unplayable::Codec("H.264 without its configuration".into()))?;
        let length_size = (avcc.length_size_minus_one & 0x3) as usize + 1;
        let mut parameter_sets = Vec::new();
        for nal in avcc
            .sequence_parameter_sets
            .iter()
            .chain(avcc.picture_parameter_sets.iter())
        {
            parameter_sets.extend_from_slice(&[0, 0, 0, 1]);
            parameter_sets.extend_from_slice(&nal.bytes);
        }
        let has_audio = reader
            .tracks()
            .values()
            .any(|t| t.track_type().ok() == Some(mp4::TrackType::Audio));
        let track = video.track_id();
        let timescale = video.timescale().max(1);
        let frames = video.sample_count();
        let description = Description {
            width: video.width() as u32,
            height: video.height() as u32,
            duration_ms: reader.duration().as_millis() as u64,
            frames,
            has_audio,
        };
        let mut demuxer = Demuxer {
            reader,
            track,
            timescale,
            length_size,
            parameter_sets,
            frames,
            order: Vec::new(),
            first_ms: 0,
            description,
        };
        demuxer.order = demuxer.presentation_order();
        demuxer.first_ms = demuxer.order.first().map(|(t, _)| *t).unwrap_or(0);
        for (t, _) in &mut demuxer.order {
            *t -= demuxer.first_ms;
        }
        Ok(demuxer)
    }

    pub fn description(&self) -> &Description {
        &self.description
    }

    /// Every sample's presentation time, sorted, without reading the bytes
    /// twice: `read_sample` hands the whole sample back, so this is one
    /// pass over the file's sample table by way of its bytes. A few
    /// hundred megabytes at most, once.
    fn presentation_order(&mut self) -> Vec<(u64, u32)> {
        let mut order = Vec::with_capacity(self.frames as usize);
        for id in 1..=self.frames {
            if let Ok(Some(s)) = self.reader.read_sample(self.track, id) {
                let composition = s.start_time as i64 + s.rendering_offset as i64;
                let ms = composition.max(0) as u64 * 1000 / self.timescale as u64;
                order.push((ms, id));
            }
        }
        order.sort();
        order
    }

    /// How many pictures there are to show.
    pub fn len(&self) -> usize {
        self.order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    /// The index in presentation order of the last keyframe at or before
    /// `ms`, in **decode** order terms: the sample id to start decoding from
    /// so that the picture at `ms` can be shown.
    pub fn keyframe_before(&mut self, ms: u64) -> u32 {
        let want = match self.order.binary_search_by(|(t, _)| t.cmp(&ms)) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        };
        let target = self.order.get(want).map(|(_, id)| *id).unwrap_or(1);
        let mut id = target;
        while id > 1 {
            if let Ok(Some(s)) = self.reader.read_sample(self.track, id)
                && s.is_sync
            {
                break;
            }
            id -= 1;
        }
        id
    }

    /// The sample with this id (1-based, decode order), as the decoder
    /// wants it. `None` past the end.
    pub fn sample(&mut self, id: u32) -> Option<Sample> {
        let s = self.reader.read_sample(self.track, id).ok().flatten()?;
        let composition = s.start_time as i64 + s.rendering_offset as i64;
        let at_ms = (composition.max(0) as u64 * 1000 / self.timescale as u64)
            .saturating_sub(self.first_ms);
        let mut annex_b = Vec::with_capacity(s.bytes.len() + self.parameter_sets.len() + 16);
        if s.is_sync {
            annex_b.extend_from_slice(&self.parameter_sets);
        }
        let mut rest = &s.bytes[..];
        while rest.len() >= self.length_size {
            let mut len = 0usize;
            for b in &rest[..self.length_size] {
                len = (len << 8) | *b as usize;
            }
            rest = &rest[self.length_size..];
            if len > rest.len() {
                break;
            }
            annex_b.extend_from_slice(&[0, 0, 0, 1]);
            annex_b.extend_from_slice(&rest[..len]);
            rest = &rest[len..];
        }
        Some(Sample {
            at_ms,
            sync: s.is_sync,
            annex_b,
        })
    }

    pub fn frames(&self) -> u32 {
        self.frames
    }
}
