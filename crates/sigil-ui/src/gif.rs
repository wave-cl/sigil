//! A gif loader that decodes off the interface thread.
//!
//! `egui_extras` ships one, and it decodes **every frame, on the thread that
//! asked, while holding its cache lock** -- unlike its still-image loader,
//! which hands the work to a background thread and answers "pending" until
//! it is done. A gif of a few megabytes is a few hundred frames, and that
//! is a window frozen for as long as they take, once per launch, because
//! decoding is not what the disc caches.
//!
//! This one is the same loader with the decode moved: the bytes go to a
//! thread, the loader answers pending, the thumbnail stays up (see
//! `attachment`), and the thread asks for a repaint when the frames are in.
//! Installed after `egui_extras`'s, so it is tried first.

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;

use egui::load::{BytesPoll, ImageLoadResult, ImageLoader, ImagePoll, LoadError, SizeHint};
use egui::mutex::Mutex;
use egui::{ColorImage, FrameDurations, Id, decode_animated_image_uri, has_gif_magic_header};
use image::AnimationDecoder as _;

/// The frames, decoded, and how long each stays up.
struct Animated {
    frames: Vec<Arc<ColorImage>>,
    durations: FrameDurations,
}

impl Animated {
    fn decode(data: &[u8]) -> Result<Self, String> {
        let decoder = image::codecs::gif::GifDecoder::new(Cursor::new(data))
            .map_err(|e| format!("Failed to decode gif: {e}"))?;
        let mut frames = Vec::new();
        let mut durations = Vec::new();
        for frame in decoder.into_frames() {
            let frame = frame.map_err(|e| format!("Failed to decode gif: {e}"))?;
            let img = frame.buffer();
            let delay: Duration = frame.delay().into();
            frames.push(Arc::new(ColorImage::from_rgba_unmultiplied(
                [img.width() as usize, img.height() as usize],
                img.as_flat_samples().as_slice(),
            )));
            durations.push(delay);
        }
        if frames.is_empty() {
            return Err("Failed to decode gif: no frames".into());
        }
        Ok(Self {
            frames,
            durations: FrameDurations::new(durations),
        })
    }

    fn bytes(&self) -> usize {
        self.frames
            .iter()
            .map(|f| f.pixels.len() * std::mem::size_of::<egui::Color32>())
            .sum()
    }

    fn frame(&self, index: usize) -> Arc<ColorImage> {
        Arc::clone(&self.frames[index % self.frames.len()])
    }
}

/// What the loader knows about one gif: being decoded, decoded, or refused.
enum Entry {
    Decoding,
    Ready(Arc<Animated>),
    Failed(String),
}

/// Shared with the threads that decode, which is why it is an `Arc` inside
/// the loader rather than the loader being one: egui hands a loader out as
/// `&self`.
#[derive(Default)]
pub struct GifLoader {
    cache: Arc<Mutex<HashMap<String, Entry>>>,
}

impl GifLoader {
    pub const ID: &'static str = egui::generate_loader_id!(GifLoader);
}

impl ImageLoader for GifLoader {
    fn id(&self) -> &str {
        Self::ID
    }

    fn load(&self, ctx: &egui::Context, frame_uri: &str, _: SizeHint) -> ImageLoadResult {
        let (uri, index) =
            decode_animated_image_uri(frame_uri).map_err(|_| LoadError::NotSupported)?;
        let mut cache = self.cache.lock();
        match cache.get(uri) {
            Some(Entry::Ready(gif)) => Ok(ImagePoll::Ready {
                image: gif.frame(index),
            }),
            Some(Entry::Decoding) => Ok(ImagePoll::Pending { size: None }),
            Some(Entry::Failed(why)) => Err(LoadError::Loading(why.clone())),
            None => match ctx.try_load_bytes(uri) {
                Ok(BytesPoll::Ready { bytes, .. }) => {
                    if !has_gif_magic_header(&bytes) {
                        return Err(LoadError::NotSupported);
                    }
                    cache.insert(uri.to_owned(), Entry::Decoding);
                    // Off this thread, and the cache lock is not held while
                    // it runs: the lock is taken again only to put the
                    // answer in. `ctx.data_mut` is the contract the widget
                    // reads frame timing through, so it is filled before the
                    // entry turns ready, and the repaint comes after both.
                    let uri = uri.to_owned();
                    let ctx = ctx.clone();
                    let cache = Arc::clone(&self.cache);
                    std::thread::Builder::new()
                        .name(format!("gif-decode-{uri}"))
                        .spawn(move || {
                            let entry = match Animated::decode(&bytes) {
                                Ok(gif) => {
                                    ctx.data_mut(|d| {
                                        *d.get_temp_mut_or_default(Id::new(&uri)) =
                                            gif.durations.clone();
                                    });
                                    Entry::Ready(Arc::new(gif))
                                }
                                Err(why) => Entry::Failed(why),
                            };
                            cache.lock().insert(uri, entry);
                            ctx.request_repaint();
                        })
                        .expect("failed to spawn a thread to decode a gif");
                    Ok(ImagePoll::Pending { size: None })
                }
                Ok(BytesPoll::Pending { size }) => Ok(ImagePoll::Pending { size }),
                Err(e) => Err(e),
            },
        }
    }

    fn forget(&self, uri: &str) {
        let _ = self.cache.lock().remove(uri);
    }

    fn forget_all(&self) {
        self.cache.lock().clear();
    }

    fn byte_size(&self) -> usize {
        self.cache
            .lock()
            .values()
            .map(|e| match e {
                Entry::Ready(gif) => gif.bytes(),
                Entry::Failed(why) => why.len(),
                Entry::Decoding => 0,
            })
            .sum()
    }
}
