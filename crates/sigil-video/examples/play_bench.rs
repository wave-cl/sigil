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
