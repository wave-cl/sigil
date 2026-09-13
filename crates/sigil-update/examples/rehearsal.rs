//! Rehearse an update of a real bundle against a release server of one's
//! own, without a window:
//!
//!     cargo run -p sigil-update --example rehearsal -- http://127.0.0.1:8765 /path/to/sigil.app
//!
//! The same calls the Desktop pane's buttons make, minus the buttons: check,
//! then Update, then print the relaunch command rather than running it. The
//! server has to answer `/releases/latest` with a newer tag and serve the
//! zip, the manifest and its signature -- signed with the key this build
//! carries, or the check ends in `Failed` and says so. See
//! docs/packaging.md, "Rehearsing an update".
use std::time::{Duration, Instant};

use sigil_update::checker::UpdateState;
use sigil_update::{Install, Updater};

fn main() {
    let api = std::env::args()
        .nth(1)
        .expect("usage: rehearsal <api base> <sigil.app>");
    let app = std::path::PathBuf::from(
        std::env::args()
            .nth(2)
            .expect("usage: rehearsal <api base> <sigil.app>"),
    );
    let install = Install::MacBundle { app };
    let updater = Updater::start(
        sigil_update::release::Client::new(api),
        sigil_update::manifest::verifying_key(&sigil_update::PUBLIC_KEY),
        install.clone(),
        Duration::ZERO,
        Duration::from_secs(3600),
        || {},
    );
    let wait = |until: &dyn Fn(&UpdateState) -> bool| {
        let deadline = Instant::now() + Duration::from_secs(120);
        let mut last = None;
        loop {
            let s = updater.state();
            if last.as_ref() != Some(&s) {
                println!("{s:?}");
                last = Some(s.clone());
            }
            if until(&s) {
                return s;
            }
            assert!(Instant::now() < deadline, "timed out");
            std::thread::sleep(Duration::from_millis(50));
        }
    };
    let s = wait(&|s| !matches!(s, UpdateState::Unknown | UpdateState::Checking));
    if !matches!(s, UpdateState::Available { .. }) {
        return;
    }
    updater.update();
    let s = wait(&|s| {
        matches!(
            s,
            UpdateState::Ready { .. }
                | UpdateState::Failed { .. }
                | UpdateState::Unreachable { .. }
        )
    });
    if matches!(s, UpdateState::Ready { .. }) {
        let target = sigil_update::relaunch::target(&install).unwrap();
        println!(
            "would relaunch with {:?}",
            sigil_update::relaunch::relaunch_command(std::process::id(), &target)
        );
    }
}
