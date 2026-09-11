//! How the sound comes out. A bench tool, not a feature.
//!   cargo run --release -p sigil-video --example sound_bench -- <file.mp4>
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let bytes: std::sync::Arc<[u8]> = std::fs::read(&a[1]).unwrap().into();
    let t = std::time::Instant::now();
    let mut sound = sigil_video::sound::Sound::open(bytes).expect("no sound");
    println!(
        "{} Hz, {} channels, opened in {:?}",
        sound.rate,
        sound.channels,
        t.elapsed()
    );
    let t = std::time::Instant::now();
    let mut chunks = 0;
    let mut frames = 0usize;
    let mut first = None;
    let mut last = 0;
    let mut peak = 0f32;
    while let Some(c) = sound.decode_next() {
        first.get_or_insert(c.at_ms);
        last = c.at_ms;
        frames += c.samples.len() / sound.channels;
        peak = c.samples.iter().fold(peak, |p, s| p.max(s.abs()));
        chunks += 1;
        if chunks >= 2000 {
            break;
        }
    }
    println!(
        "{chunks} chunks, {frames} frames ({:.1} s of sound, {:?}..{last} ms, peak {peak:.2}) decoded in {:?}",
        frames as f64 / sound.rate as f64,
        first,
        t.elapsed()
    );
    let t = std::time::Instant::now();
    sound.seek(200_000);
    let c = sound.decode_next().unwrap();
    println!(
        "seek to 200 s: first chunk at {} ms in {:?}",
        c.at_ms,
        t.elapsed()
    );
}
