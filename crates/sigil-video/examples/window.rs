//! Play a file in a real egui window, the way sigil does, and print how
//! the pictures landed on the window's frames. A bench tool, not a feature:
//! the only honest instrument for jitter is the window.
//!
//!   cargo run --release -p sigil-video --example window -- <file.mp4> [seconds]
use std::time::Instant;

struct App {
    player: sigil_video::Player,
    texture: Option<egui::TextureHandle>,
    shown: Option<u64>,
    started: Instant,
    /// (wall ms, position ms, picture ms) at each change of picture.
    landed: Vec<(u64, u64, u64)>,
    /// Wall ms of every repaint.
    repaints: Vec<u64>,
    secs: u64,
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let ctx = &ctx;
        let wall = self.started.elapsed().as_millis() as u64;
        self.repaints.push(wall);
        if let Some((at, image)) = self.player.frame()
            && self.shown != Some(at)
        {
            let data = egui::ImageData::Color(image);
            match &mut self.texture {
                Some(t) => t.set(data, egui::TextureOptions::LINEAR),
                None => {
                    self.texture = Some(ctx.load_texture("v", data, egui::TextureOptions::LINEAR))
                }
            }
            self.shown = Some(at);
            self.landed.push((wall, self.player.position_ms(), at));
        }
        if let Some(t) = &self.texture {
            let size = t.size_vec2();
            let scale = (ui.available_width() / size.x).min(ui.available_height() / size.y);
            ui.add(
                egui::Image::from_texture(egui::load::SizedTexture::from_handle(t))
                    .fit_to_exact_size(size * scale),
            );
        }
        if wall > self.secs * 1000 {
            self.report();
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

impl App {
    fn report(&self) {
        let mut gaps: Vec<i64> = self
            .repaints
            .windows(2)
            .map(|w| (w[1] - w[0]) as i64)
            .collect();
        gaps.sort();
        let n = gaps.len();
        println!(
            "{} repaints; gap ms: min {} median {} p90 {} p99 {} max {}",
            n + 1,
            gaps[0],
            gaps[n / 2],
            gaps[n * 9 / 10],
            gaps[n * 99 / 100],
            gaps[n - 1]
        );
        let mut late: Vec<i64> = self
            .landed
            .iter()
            .map(|(_, pos, at)| *pos as i64 - *at as i64)
            .collect();
        let mut between: Vec<i64> = self
            .landed
            .windows(2)
            .map(|w| (w[1].0 - w[0].0) as i64)
            .collect();
        let steps: Vec<i64> = self
            .landed
            .windows(2)
            .map(|w| (w[1].2 - w[0].2) as i64)
            .collect();
        let skipped = steps.iter().filter(|s| **s > 40).count();
        late.sort();
        between.sort();
        let n = late.len();
        let m = between.len();
        println!(
            "{n} pictures shown ({skipped} steps skipped a picture); shown late by ms: median {} p90 {} max {} | wall between pictures ms: min {} median {} p90 {} p99 {} max {}",
            late[n / 2],
            late[n * 9 / 10],
            late[n - 1],
            between[0],
            between[m / 2],
            between[m * 9 / 10],
            between[m * 99 / 100],
            between[m - 1]
        );
        // The worst stretch, in full.
        if let Some((i, _)) = self
            .landed
            .windows(2)
            .enumerate()
            .max_by_key(|(_, w)| w[1].0 - w[0].0)
        {
            let lo = i.saturating_sub(3);
            let hi = (i + 4).min(self.landed.len());
            println!("around the worst gap (wall, position, picture):");
            for (w, p, a) in &self.landed[lo..hi] {
                println!("  {w:>6} {p:>6} {a:>6}");
            }
        }
    }
}

fn main() -> eframe::Result {
    let a: Vec<String> = std::env::args().collect();
    let bytes: std::sync::Arc<[u8]> = std::fs::read(&a[1]).unwrap().into();
    let secs: u64 = a.get(2).and_then(|s| s.parse().ok()).unwrap_or(8);
    eframe::run_native(
        "video window bench",
        eframe::NativeOptions::default(),
        Box::new(move |cc| {
            if std::env::var("DEVICE").is_ok() {
                std::thread::spawn(|| {
                    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
                    let t = Instant::now();
                    let host = cpal::default_host();
                    let device = host.default_output_device().unwrap();
                    eprintln!("device in {:?}", t.elapsed());
                    let config = device.default_output_config().unwrap().config();
                    eprintln!("config in {:?}", t.elapsed());
                    let first = std::sync::Arc::new(std::sync::Mutex::new(None));
                    let f = first.clone();
                    let stream = device
                        .build_output_stream(
                            config,
                            move |out: &mut [f32], _| {
                                out.fill(0.0);
                                f.lock().unwrap().get_or_insert(t.elapsed());
                            },
                            |_| {},
                            None,
                        )
                        .unwrap();
                    eprintln!("built in {:?}", t.elapsed());
                    stream.play().unwrap();
                    std::thread::sleep(std::time::Duration::from_millis(3000));
                    eprintln!("first callback at {:?}", first.lock().unwrap());
                });
            }
            let player = sigil_video::Player::open(bytes, cc.egui_ctx.clone()).unwrap();
            player.set_volume(0.0);
            player.play();
            Ok(Box::new(App {
                player,
                texture: None,
                shown: None,
                started: Instant::now(),
                landed: Vec::new(),
                repaints: Vec::new(),
                secs,
            }))
        }),
    )
}
