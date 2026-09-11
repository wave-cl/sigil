//! Video in the transcript: an MP4 attachment, decoded here and drawn as
//! frames, with its sound.
pub mod demux;
pub mod picture;
pub mod player;
pub mod sound;

pub use demux::{Demuxer, Description, Unplayable};
pub use player::Player;
