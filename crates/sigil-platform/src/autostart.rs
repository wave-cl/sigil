//! Starting sigil at login.
//!
//! Off by default and always somebody's decision. A program that arranges to
//! run forever without being asked has taken something that was not offered.

use crate::support::Support;
use auto_launch::{AutoLaunch, MacOSLaunchMode};

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
            Ok(path) => {
                // A Launch Agent plist on macOS rather than AppleScript: the
                // AppleScript route adds a login *item*, which needs the app to
                // be a bundle and puts sigil in a list somebody did not expect
                // to be editing. A plist under ~/Library/LaunchAgents is
                // sigil's own file and removing it is the whole undo.
                let inner = AutoLaunch::new(
                    "sigil",
                    &path.to_string_lossy(),
                    MacOSLaunchMode::LaunchAgent,
                    &[] as &[&str],
                    &[] as &[&str],
                    "",
                );
                Autostart {
                    inner: Some(inner),
                    support: Support::Yes,
                }
            }
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
