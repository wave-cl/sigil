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

        player.play();
        std::thread::sleep(Duration::from_millis(700));
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
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(player.ended(), "{name}: never ended");
        assert!(!player.playing());
    }
}

/// The same, on the sound's clock, through the output device. Not run by
/// default: CoreAudio inside `cargo test` on macOS takes ten seconds to
/// answer the first device query, and a runner has no device at all.
///
///   cargo test -p sigil-video --test playback -- --ignored
#[test]
#[ignore]
fn with_the_device_the_sound_is_the_clock() {
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
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(player.ended());
}
