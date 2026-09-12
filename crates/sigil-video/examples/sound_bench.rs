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
    // To the end, then back to the start: what "play again" does. (The
    // 2000-chunk loop above already reached the end of a short clip.)
    while sound.decode_next().is_some() {}
    sound.seek(0);
    match sound.decode_next() {
        Some(c) => println!("after the end, seek to 0: first chunk at {} ms", c.at_ms),
        None => println!("after the end, seek to 0: NOTHING -- the reader is stuck at the end"),
    }
    sound.seek(1_000);
    match sound.decode_next() {
        Some(c) => println!("then seek to 1 s: first chunk at {} ms", c.at_ms),
        None => println!("then seek to 1 s: NOTHING"),
    }
}
