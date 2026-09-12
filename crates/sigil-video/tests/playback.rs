//! A file plays: pictures come out in order and on the clock, sound
//! decodes, seeking lands, and the end is an end.
//!
//! Two fixtures, two seconds each, made by ffmpeg from its test pattern:
//! one with a 440 Hz tone (H.264 High with B-frames, AAC mono at 44.1 kHz,
//! which is not what any output device runs at, so the resampler is in the
//! path), one silent. On a machine with no output device -- every CI runner
//! -- the first plays on the wall clock like the second; the sound path
//! itself is covered by decoding, and by ear.

use std::sync::Arc;
use std::time::{Duration, Instant};

use sigil_video::picture::{Ordered, Pictures};
use sigil_video::sound::Sound;
use sigil_video::{Demuxer, Player, Unplayable};

fn fixture(name: &str) -> Arc<[u8]> {
    std::fs::read(format!(
        "{}/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
    .into()
}

#[test]
fn the_pictures_come_out_in_order_and_on_time() {
    let demuxer = Demuxer::open(fixture("two_seconds.mp4")).unwrap();
    let d = demuxer.description().clone();
    assert_eq!((d.width, d.height), (96, 64));
    assert!((1900..=2100).contains(&d.duration_ms), "{}", d.duration_ms);
    assert_eq!(d.frames, 60);
    assert!(d.has_audio);
    let mut pictures = Ordered::new(Pictures::new(demuxer).unwrap());
    let mut times = Vec::new();
    while let Some(f) = pictures.decode_next() {
        assert_eq!(f.image.size, [96, 64]);
        times.push(f.at_ms);
    }
    assert_eq!(times.len(), 60, "every picture, once");
    assert!(
        times.windows(2).all(|w| w[0] < w[1]),
        "out of order: {times:?}"
    );
    // Thirty a second: consecutive pictures 33 ms apart, give or take the
    // millisecond the timescale rounds.
    assert!(
        times.windows(2).all(|w| (32..=34).contains(&(w[1] - w[0]))),
        "uneven: {times:?}"
    );
    // The test pattern moves, so two pictures are not the same picture.
    let mut again =
        Ordered::new(Pictures::new(Demuxer::open(fixture("two_seconds.mp4")).unwrap()).unwrap());
    let first = again.decode_next().unwrap().image;
    let later = again.decode_next().unwrap().image;
    assert_ne!(
        first.pixels, later.pixels,
        "the decoder handed back one picture twice"
    );
}

#[test]
fn a_seek_lands_on_the_picture_asked_for() {
    let demuxer = Demuxer::open(fixture("two_seconds.mp4")).unwrap();
    let mut pictures = Ordered::new(Pictures::new(demuxer).unwrap());
    pictures.seek(1_000);
    let f = pictures.decode_next().unwrap();
    // The keyframe interval is fifteen pictures, so the seek starts up to
    // half a second early and decodes forward; what comes out first is
    // the keyframe, and the caller walks to the target from there.
    assert!(f.at_ms <= 1_000 && f.at_ms >= 500, "landed at {}", f.at_ms);
    let mut f = f;
    while f.at_ms < 1_000 {
        f = pictures.decode_next().unwrap();
    }
    assert!((1_000..=1_034).contains(&f.at_ms), "walked to {}", f.at_ms);
}

#[test]
fn the_sound_decodes_to_a_tone() {
    let mut sound = Sound::open(fixture("two_seconds.mp4")).expect("no sound");
    assert_eq!((sound.rate, sound.channels), (44_100, 1));
    let mut all = Vec::new();
    while let Some(c) = sound.decode_next() {
        all.extend(c.samples);
    }
    let seconds = all.len() as f64 / 44_100.0;
    assert!((1.9..=2.1).contains(&seconds), "{seconds} s of sound");
    // A 440 Hz tone crosses zero 880 times a second. Counted over the
    // middle second, clear of the encoder's fade-in and padding.
    let middle = &all[22_050..66_150];
    let crossings = middle
        .windows(2)
        .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
        .count();
    assert!(
        (860..=900).contains(&crossings),
        "{crossings} zero crossings a second"
    );
}

#[test]
fn a_silent_file_has_no_sound_and_still_describes_itself() {
    assert!(Sound::open(fixture("silent.mp4")).is_none());
    let d = Demuxer::open(fixture("silent.mp4"))
        .unwrap()
        .description()
        .clone();
    assert!(!d.has_audio);
    assert_eq!(d.frames, 60);
}

#[test]
fn something_that_is_not_a_video_says_so() {
    let Err(err) = Demuxer::open(Arc::from(&b"not a video at all"[..])) else {
        panic!("opened as a video");
    };
    assert!(matches!(err, Unplayable::NotMp4(_)), "{err}");
}

/// The player keeps time and shows the picture that is due.
///
/// On the wall clock, without the output device: see `Player::open_with`
/// for why a test process cannot count on one. Both fixtures, so the file
/// with a soundtrack is shown to play its pictures the same way when the
/// sound is not wanted.
#[test]
fn the_player_advances_while_playing_and_holds_while_paused() {
    for name in ["silent.mp4", "two_seconds.mp4"] {
        let ctx = egui::Context::default();
        let player = Player::open_with(fixture(name), ctx, false).unwrap();
        player.set_volume(0.0);
        // The first picture is up before anything plays.
        let deadline = Instant::now() + Duration::from_secs(5);
        while player.frame().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        let (at, _) = player.frame().expect("no first picture");
        assert!(at < 40, "{name}: the first picture is at {at} ms");
        assert_eq!(player.position_ms(), 0);

        // Played the way the window plays it: `frame` asked for at each
        // repaint, which is what takes pictures off the queue as they
        // fall due.
        player.play();
        let until = Instant::now() + Duration::from_millis(700);
        while Instant::now() < until {
            let _ = player.frame();
            std::thread::sleep(Duration::from_millis(5));
        }
        player.pause();
        let pos = player.position_ms();
        assert!(
            (500..=1_000).contains(&pos),
            "{name}: played for 700 ms and the position is {pos} ms"
        );
        let (shown, _) = player.frame().unwrap();
        assert!(
            shown <= pos && shown + 100 >= pos,
            "{name}: at {pos} ms the picture on screen is the one for {shown} ms"
        );
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(player.position_ms(), pos, "{name}: moved while paused");

        player.seek(1_500);
        let deadline = Instant::now() + Duration::from_secs(5);
        while player.frame().map(|(at, _)| at < 1_400).unwrap_or(true) && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(10));
        }
        let (at, _) = player.frame().unwrap();
        assert!(
            (1_450..=1_550).contains(&at),
            "{name}: seek to 1500 showed {at}"
        );

        // Play to the end: it stops there and says so.
        player.play();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !player.ended() && Instant::now() < deadline {
            let _ = player.frame();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(player.ended(), "{name}: never ended");
        assert!(!player.playing());

        // And again from the start: play after the end is a seek to nought
        // and off it goes. It stopped dead the first time -- the sound
        // reader would not seek back once it had reached the end, and the
        // threads called the end again on a flag the seek had not yet
        // reset.
        player.play();
        let until = Instant::now() + Duration::from_millis(600);
        while Instant::now() < until {
            let _ = player.frame();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(player.playing(), "{name}: play after the end did not play");
        assert!(!player.ended(), "{name}: ended again at once");
        let again = player.position_ms();
        assert!(
            (300..=900).contains(&again),
            "{name}: played again for 600 ms and the position is {again} ms"
        );
    }
}

/// The same, on the sound's clock, through the output device. Opt-in:
/// CoreAudio inside `cargo test` on macOS takes ten seconds to answer the
/// first device query, and a runner has no device at all. Not `#[ignore]`,
/// because `scripts/snapshot-test` runs every ignored test in the
/// workspace and this is not one it should.
///
///   SIGIL_VIDEO_DEVICE=1 cargo test --release -p sigil-video --test playback with_the_device
#[test]
fn with_the_device_the_sound_is_the_clock() {
    if std::env::var_os("SIGIL_VIDEO_DEVICE").is_none() {
        eprintln!("skipped: set SIGIL_VIDEO_DEVICE=1 to run against the output device");
        return;
    }
    let ctx = egui::Context::default();
    let player = Player::open(fixture("two_seconds.mp4"), ctx).unwrap();
    player.set_volume(0.0);
    player.play();
    let deadline = Instant::now() + Duration::from_secs(30);
    while player.position_ms() == 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let started = Instant::now();
    std::thread::sleep(Duration::from_millis(1_000));
    let pos = player.position_ms();
    let wall = started.elapsed().as_millis() as u64;
    assert!(
        pos.abs_diff(wall) < 150,
        "a second of wall time was {pos} ms of sound"
    );
    while !player.ended() && Instant::now() < deadline {
        let _ = player.frame();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(player.ended());
    // And again, with the device: the sound reader is made afresh.
    player.play();
    let until = Instant::now() + Duration::from_millis(800);
    while Instant::now() < until {
        let _ = player.frame();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        player.playing() && !player.ended(),
        "play after the end did not play"
    );
    assert!(
        player.position_ms() > 200,
        "no sound played the second time"
    );
}

/// Picture *k* is display frame *k* -- the same picture ffmpeg decodes
/// there, not a neighbour of it.
///
/// The decoder hands pictures back in display order and says nothing about
/// which sample each was. Labelling them by the sample just fed (decode
/// order) and sorting on that put a permutation of neighbouring pictures on
/// screen, every one on time: pure judder, invisible to every timing
/// measurement. Five reference frames decoded by ffmpeg, compared by PSNR:
/// the right picture is about 33 dB (the two decoders convert colour a
/// little differently, and the pattern has hard colour edges), either
/// neighbour about 21. The fixture is a moving test pattern, so neighbours
/// differ, which is what lets an order be told from a shuffle.
#[test]
fn each_picture_is_the_one_ffmpeg_decodes_there() {
    let demuxer = Demuxer::open(fixture("two_seconds.mp4")).unwrap();
    let mut pictures = Ordered::new(Pictures::new(demuxer).unwrap());
    let mut mine = Vec::new();
    while let Some(f) = pictures.decode_next() {
        mine.push(f);
    }
    assert_eq!(mine.len(), 60);
    // **By the time each picture claims**, not by the order they came out:
    // the fault was in the labels, and the pictures came out in the right
    // order all along.
    let labelled = |n: usize| -> &egui::ColorImage {
        let want = n as u64 * 1000 / 30;
        &mine
            .iter()
            .min_by_key(|f| f.at_ms.abs_diff(want))
            .unwrap()
            .image
    };
    for n in [10usize, 21, 33, 44, 57] {
        let reference = image::open(format!(
            "{}/tests/fixtures/frames/{n}.png",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap()
        .to_rgb8();
        let ours = labelled(n);
        assert_eq!(ours.size, [96, 64]);
        let db = psnr(ours, &reference);
        let near = psnr(labelled(n + 1), &reference);
        let before = psnr(labelled(n - 1), &reference);
        assert!(
            db > 30.0,
            "picture {n} is not the picture ffmpeg decodes there: {db:.1} dB \
             (neighbours {before:.1} / {near:.1})"
        );
        // And both neighbours are measurably different pictures, or this
        // test could not tell an order from a shuffle.
        assert!(
            near < 26.0 && before < 26.0,
            "pictures around {n} look alike ({before:.1} / {near:.1} dB)"
        );
    }
}

fn psnr(a: &egui::ColorImage, b: &image::RgbImage) -> f64 {
    let mut err = 0.0f64;
    for (i, p) in a.pixels.iter().enumerate() {
        let q = b.get_pixel((i % 96) as u32, (i / 96) as u32);
        for (x, y) in [(p.r(), q[0]), (p.g(), q[1]), (p.b(), q[2])] {
            let d = x as f64 - y as f64;
            err += d * d;
        }
    }
    let mse = err / (a.pixels.len() as f64 * 3.0);
    if mse == 0.0 {
        99.0
    } else {
        10.0 * (255.0f64 * 255.0 / mse).log10()
    }
}
