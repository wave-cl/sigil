//! Starting sigil at login.
//!
//! Off by default and always somebody's decision. A program that arranges to
//! run forever without being asked has taken something that was not offered.

use crate::support::Support;
use auto_launch::AutoLaunch;

/// Build the platform's autostart entry.
///
/// `AutoLaunch::new` **takes different arguments on each platform** — six on
/// macOS, four on Linux — so there is no one call that compiles for both. The
/// crate says so in a doc comment and nowhere the type checker can reach, which
/// is why the macOS-shaped call compiled happily here and failed the moment CI
/// tried it on Linux. That was the first thing cross-platform CI caught.
#[cfg(target_os = "macos")]
fn build(name: &str, path: &str) -> AutoLaunch {
    // A Launch Agent plist rather than AppleScript. The AppleScript route adds
    // a login *item*, which needs the app to be a bundle and puts sigil in a
    // list somebody did not expect to be editing. A plist under
    // ~/Library/LaunchAgents is sigil's own file, and removing it is the whole
    // undo.
    AutoLaunch::new(
        name,
        path,
        auto_launch::MacOSLaunchMode::LaunchAgent,
        &[] as &[&str],
        &[] as &[&str],
        "",
    )
}

/// See the macOS version above for why this is a separate function.
#[cfg(not(target_os = "macos"))]
fn build(name: &str, path: &str) -> AutoLaunch {
    // A `.desktop` file under ~/.config/autostart rather than a systemd user
    // unit. Every desktop reads the first; the second needs a running systemd
    // user instance, which is usual and not universal. And it is the same
    // mechanism the packages already use for the launcher entry, so there is
    // one kind of file to understand rather than two.
    AutoLaunch::new(
        name,
        path,
        auto_launch::LinuxLaunchMode::XdgAutostart,
        &[] as &[&str],
    )
}

pub struct Autostart {
    inner: Option<AutoLaunch>,
    support: Support,
}

impl Default for Autostart {
    fn default() -> Self {
        Self::new()
    }
}

impl Autostart {
    pub fn new() -> Autostart {
        match std::env::current_exe() {
            Ok(path) => Autostart {
                inner: Some(build("sigil", &path.to_string_lossy())),
                support: Support::Yes,
            },
            Err(e) => Autostart {
                inner: None,
                support: Support::no(format!("cannot find sigil's own path: {e}")),
            },
        }
    }

    pub fn support(&self) -> &Support {
        &self.support
    }

    pub fn enabled(&self) -> bool {
        self.inner
            .as_ref()
            .and_then(|a| a.is_enabled().ok())
            .unwrap_or(false)
    }

    pub fn set(&self, on: bool) -> Result<(), String> {
        let Some(inner) = &self.inner else {
            return Err(self.support.reason().unwrap_or("unavailable").to_string());
        };
        if on { inner.enable() } else { inner.disable() }.map_err(|e| e.to_string())
    }
}
