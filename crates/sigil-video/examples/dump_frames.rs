//! Write pictures N..M of a file as PNGs, to compare against another
//! decoder's. A bench tool, not a feature: it is how a wrong picture order
//! was found when every timing number said the playback was perfect.
//!   cargo run --release -p sigil-video --example dump_frames -- <file.mp4> <from> <to> <dir>
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let bytes: std::sync::Arc<[u8]> = std::fs::read(&a[1]).unwrap().into();
    let (from, to): (usize, usize) = (a[2].parse().unwrap(), a[3].parse().unwrap());
    let out = &a[4];
    let demuxer = sigil_video::Demuxer::open(bytes).unwrap();
    let mut pictures = sigil_video::picture::Pictures::new(demuxer).unwrap();
    let mut i = 0;
    while let Some(f) = pictures.decode_next() {
        if i >= from && i < to {
            let [w, h] = f.image.size;
            let rgba: Vec<u8> = f.image.pixels.iter().flat_map(|p| p.to_array()).collect();
            image::RgbaImage::from_raw(w as u32, h as u32, rgba)
                .unwrap()
                .save(format!("{out}/{i:04}.png"))
                .unwrap();
        }
        i += 1;
        if i >= to {
            break;
        }
    }
}
