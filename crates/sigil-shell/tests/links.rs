//! Every link in sigil is a control somebody can press.
//!
//! # Why a dependency is what this asserts
//!
//! `ui.hyperlink_to` does not open anything itself. egui emits an
//! `OutputCommand::OpenUrl` and the backend acts on it -- and `egui-winit`'s
//! handler is
//!
//! ```text
//! fn open_url_in_browser(_url: &str) {
//!     #[cfg(feature = "webbrowser")] ...
//!     #[cfg(not(feature = "webbrowser"))] log::warn!("Cannot open url ...");
//! }
//! ```
//!
//! That feature comes from eframe's `links`, which is in eframe's *defaults*
//! -- and sigil builds eframe with `default-features = false`, for wgpu and
//! for the window backends. `links` was not named, so every link in sigil
//! compiled to a log line: the update check offers "See the release" and
//! "What changed" against a real GitHub URL, and pressing either did nothing
//! at all, on every platform, in silence. There is no failing render and no
//! error; a link that does nothing looks exactly like one nobody pressed.
//!
//! Nothing in a `cargo test` can press a link and watch a browser open. What
//! it *can* do is ask whether the crate that does the opening is in the build
//! at all, which is the whole of what went wrong.
//!
//! # And `cargo metadata` is the wrong question
//!
//! The first version of this read `cargo metadata` for the name. It passed
//! with the feature taken back out, because metadata lists every package in
//! the **lock file** whether the feature resolution reaches it or not -- and
//! the lock still held `webbrowser` from the run before. `cargo tree -i`
//! walks the resolved graph and exits 101 when nothing depends on the
//! package, which is the difference between "it is written down somewhere"
//! and "it is compiled in".

use std::process::Command;

#[test]
fn the_links_feature_is_on() {
    let out = Command::new(env!("CARGO"))
        .args(["tree", "-i", "webbrowser", "-e", "normal"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo tree runs");
    assert!(
        out.status.success(),
        "nothing in this build depends on `webbrowser`, so eframe's `links` \
         feature is off and every hyperlink in sigil opens nothing. Name \
         \"links\" in the workspace's eframe features.\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    // And that the answer is about this workspace rather than an empty one.
    let said = String::from_utf8_lossy(&out.stdout);
    assert!(
        said.contains("egui-winit"),
        "`webbrowser` is in the build but not through egui-winit, which is \
         the one that opens a link: {said}"
    );
}
