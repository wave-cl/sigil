//! Global shortcuts — the ones that work while sigil is not focused.
//!
//! # Wayland genuinely cannot do this
//!
//! An X11 client can grab a key combination for itself. Wayland has no
//! equivalent, deliberately: letting any client silently watch the keyboard is
//! the thing the design set out to prevent. The replacement is the XDG
//! `GlobalShortcuts` portal, which is new and unevenly implemented.
//!
//! So on Wayland this reports unavailable with that as the reason, and the
//! interface disables push-to-talk rather than offering a control that does
//! nothing. A mute key that silently fails is worse than no mute key: somebody
//! believes they are muted.

use crate::support::{Session, Support};
use global_hotkey::hotkey::{Code, HotKey, Modifiers};
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager};

pub struct Hotkeys {
    /// Held for its `Drop`: releasing the manager unregisters every key it
    /// took, so the binding lives exactly as long as this does. Nothing reads
    /// it back — events arrive on a process-wide channel instead.
    _manager: Option<GlobalHotKeyManager>,
    support: Support,
    mute: Option<u32>,
}

impl Default for Hotkeys {
    fn default() -> Self {
        Self::new()
    }
}

impl Hotkeys {
    pub fn new() -> Hotkeys {
        if let Support::No(why) = probe() {
            return Hotkeys {
                _manager: None,
                support: Support::No(why),
                mute: None,
            };
        }
        match GlobalHotKeyManager::new() {
            Ok(manager) => {
                let key = HotKey::new(Some(Modifiers::CONTROL | Modifiers::SHIFT), Code::KeyM);
                match manager.register(key) {
                    Ok(()) => Hotkeys {
                        mute: Some(key.id()),
                        _manager: Some(manager),
                        support: Support::Yes,
                    },
                    Err(e) => Hotkeys {
                        _manager: Some(manager),
                        support: Support::no(format!("could not take ctrl+shift+M: {e}")),
                        mute: None,
                    },
                }
            }
            Err(e) => Hotkeys {
                _manager: None,
                support: Support::no(format!("{e}")),
                mute: None,
            },
        }
    }

    pub fn support(&self) -> &Support {
        &self.support
    }

    /// How many times the mute key was pressed since the last look.
    ///
    /// Drained rather than delivered, because the events arrive on a
    /// process-wide channel with no notion of who wants them.
    pub fn mute_presses(&self) -> usize {
        if self.mute.is_none() {
            return 0;
        }
        let mut n = 0;
        while GlobalHotKeyEvent::receiver().try_recv().is_ok() {
            n += 1;
        }
        n
    }
}

fn probe() -> Support {
    match Session::detect() {
        Session::Headless => Support::no("there is no desktop session here"),
        // Carbon's RegisterEventHotKey, which needs no Accessibility
        // permission -- unlike an event tap. Verified by running it.
        Session::MacOs => Support::Yes,
        Session::X11 => Support::Yes,
        Session::Wayland => Support::no(
            "Wayland has no way for an application to claim a key combination; \
             the XDG GlobalShortcuts portal would be needed and sigil does not \
             use it yet. Shortcuts still work while sigil is focused.",
        ),
    }
}
