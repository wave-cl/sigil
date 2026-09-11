//! How fast the pictures come out of a file. A bench tool, not a feature.
//!   cargo run --release -p sigil-video --example decode_bench -- <file.mp4> [frames]
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let bytes: std::sync::Arc<[u8]> = std::fs::read(&a[1]).unwrap().into();
    let want: usize = a.get(2).and_then(|n| n.parse().ok()).unwrap_or(300);
    let t = std::time::Instant::now();
    let demuxer = sigil_video::Demuxer::open(bytes).unwrap();
    println!("{:?} opened in {:?}", demuxer.description(), t.elapsed());
    let mut pictures =
        sigil_video::picture::Ordered::new(sigil_video::picture::Pictures::new(demuxer).unwrap());
    let t = std::time::Instant::now();
    let mut n = 0;
    let mut first = None;
    let mut last = 0;
    let mut out_of_order = 0;
    let mut shown = Vec::new();
    while let Some(f) = pictures.decode_next() {
        if shown.len() < 8 {
            shown.push(f.at_ms);
        }
        first.get_or_insert(f.at_ms);
        if f.at_ms < last {
            out_of_order += 1;
        }
        last = f.at_ms;
        n += 1;
        if n >= want {
            break;
        }
    }
    println!(
        "{n} frames ({:?}..{last} ms, {out_of_order} out of order) in {:?} = {:.0} fps",
        first,
        t.elapsed(),
        n as f64 / t.elapsed().as_secs_f64()
    );
    println!("first pictures at: {shown:?}");
    let p = pictures.pictures();
    println!(
        "skipped: {} empty, {} errors, last {:?}",
        p.skipped_empty, p.skipped_err, p.last_err
    );
    let t = std::time::Instant::now();
    pictures.seek(200_000);
    let f = pictures.decode_next().unwrap();
    println!(
        "seek to 200 s: first picture at {} ms in {:?}",
        f.at_ms,
        t.elapsed()
    );
    if let Some(out) = a.get(3) {
        let [w, h] = f.image.size;
        let rgba: Vec<u8> = f.image.pixels.iter().flat_map(|p| p.to_array()).collect();
        image::RgbaImage::from_raw(w as u32, h as u32, rgba)
            .unwrap()
            .save(out)
            .unwrap();
        println!("wrote {out}");
    }
}
