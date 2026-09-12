//! Play a file headless and print the clock every 100 ms. A bench tool.
//!   cargo run --release -p sigil-video --example play_bench -- <file.mp4> [seconds]
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let bytes: std::sync::Arc<[u8]> = std::fs::read(&a[1]).unwrap().into();
    let secs: u64 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(3);
    let ctx = egui::Context::default();
    let player = sigil_video::Player::open(bytes, ctx).unwrap();
    player.set_volume(0.2);
    println!("{:?}", player.description());
    player.play();
    let t = std::time::Instant::now();
    if std::env::var("JITTER").is_ok() {
        // Poll `frame` the way a window repainting at 500 Hz would, and
        // measure how far from its due time each picture is first shown.
        // Polled at the display's rate, sixteen milliseconds, which is
        // what the window does while a video plays.
        let mut last = None;
        let mut late = Vec::new();
        let mut gaps = Vec::new();
        let mut last_shown = t;
        while t.elapsed().as_secs() < secs {
            if let Some((at, _)) = player.frame()
                && last != Some(at)
            {
                last = Some(at);
                let shown = player.position_ms();
                late.push(shown as i64 - at as i64);
                gaps.push(last_shown.elapsed().as_millis() as i64);
                last_shown = std::time::Instant::now();
            }
            std::thread::sleep(std::time::Duration::from_millis(16));
        }
        late.sort();
        gaps.sort();
        let n = late.len();
        println!(
            "{n} pictures shown; lateness ms: min {} median {} p90 {} max {} | wall gap between pictures ms: min {} median {} p90 {} max {}",
            late[0],
            late[n / 2],
            late[n * 9 / 10],
            late[n - 1],
            gaps[1],
            gaps[n / 2],
            gaps[n * 9 / 10],
            gaps[n - 1]
        );
        return;
    }
    let mut sought = false;
    while t.elapsed().as_secs() < secs {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if let Some(to) = a.get(3).and_then(|s| s.parse::<u64>().ok())
            && !sought
            && t.elapsed().as_millis() > 300
        {
            player.seek(to);
            sought = true;
            println!("-- seek to {to}");
        }
        println!(
            "wall {:>5} ms  position {:>6} ms  frame {:?}  playing {} ended {}",
            t.elapsed().as_millis(),
            player.position_ms(),
            player.frame().map(|(at, _)| at),
            player.playing(),
            player.ended()
        );
    }
}
