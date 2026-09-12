//! Video in the transcript: an MP4 attachment, decoded here and drawn as
//! frames, with its sound.
pub mod demux;
pub mod picture;
pub mod player;
pub mod sound;

pub use demux::{Demuxer, Description, Unplayable};
pub use player::Player;

/// What a file is and its first picture, for the sender to describe it and
/// make a thumbnail of it before it is sent.
pub fn still(bytes: std::sync::Arc<[u8]>) -> Result<(Description, egui::ColorImage), Unplayable> {
    let demuxer = Demuxer::open(bytes)?;
    let description = demuxer.description().clone();
    let mut pictures =
        picture::Ordered::new(picture::Pictures::new(demuxer).map_err(Unplayable::Codec)?);
    let frame = pictures
        .decode_next()
        .ok_or_else(|| Unplayable::Codec("no picture decodes".into()))?;
    Ok((description, frame.image))
}
