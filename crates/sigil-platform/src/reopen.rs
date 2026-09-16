//! The Dock, on macOS: a press on the icon brings the window up.
//!
//! Closing the window puts sigil in the tray and hides the window, and the
//! Dock icon stays -- so pressing it is the most obvious way back, and it
//! did nothing. AppKit asks the application's delegate
//! `applicationShouldHandleReopen:hasVisibleWindows:` on a Dock press, and
//! winit's delegate does not answer it; the default is to activate the
//! application, which for a hidden window is nothing anybody can see. The
//! method is added to winit's delegate class here, at run time, and reports
//! the press the way the tray's Open does.
//!
//! The application becoming active -- Cmd-Tab to it, or a Dock press while
//! it is inactive -- is watched as well, for the same reason: an application
//! brought to the front with its only window hidden has been brought to
//! nothing. Presenting a window that is already up is harmless.

use std::ffi::CStr;
use std::ptr::NonNull;

use objc2::runtime::{AnyClass, AnyObject, Bool, Sel};
use objc2::{MainThreadMarker, sel};
use objc2_foundation::{NSNotification, NSNotificationCenter};

use crate::support::Support;
use crate::tray::{TrayAction, report};

/// winit's delegate class, by the name it declares. A later winit that
/// renames it is reported, not guessed at.
const DELEGATE: &CStr = c"WinitApplicationDelegate";

extern "C-unwind" fn should_handle_reopen(
    _this: &AnyObject,
    _sel: Sel,
    _app: &AnyObject,
    _has_visible_windows: Bool,
) -> Bool {
    report(TrayAction::Open);
    // Let AppKit do what it would have done as well: activate.
    Bool::YES
}

/// Install both hooks. **Main thread**, after winit has made its delegate,
/// which is anywhere inside eframe's creator.
pub fn watch() -> Support {
    if MainThreadMarker::new().is_none() {
        return Support::no("the Dock can only be watched from the main thread");
    }
    let Some(class) = AnyClass::get(DELEGATE) else {
        return Support::no(format!(
            "winit's application delegate is not called {}",
            DELEGATE.to_string_lossy()
        ));
    };
    // SAFETY: the selector, the imp's signature and the type encoding
    // agree -- `applicationShouldHandleReopen:hasVisibleWindows:` takes the
    // application and a BOOL and answers a BOOL -- and the class is winit's
    // own, which implements no such method to be clobbered.
    let added = unsafe {
        objc2::ffi::class_addMethod(
            class as *const AnyClass as *mut AnyClass,
            sel!(applicationShouldHandleReopen:hasVisibleWindows:),
            std::mem::transmute::<
                extern "C-unwind" fn(&AnyObject, Sel, &AnyObject, Bool) -> Bool,
                objc2::runtime::Imp,
            >(should_handle_reopen),
            c"B@:@B".as_ptr(),
        )
    };
    if !added.as_bool() {
        return Support::no("winit's delegate already answers the Dock, and differently");
    }
    // The activation, by notification: the delegate's own method for it is
    // wired up when the delegate is set, so adding one now would go unheard.
    let block = block2::RcBlock::new(|_: NonNull<NSNotification>| {
        report(TrayAction::Open);
    });
    // SAFETY: the block takes the notification and reads nothing from it;
    // no object and no queue means every sender, on the posting thread,
    // which for this notification is the main thread.
    let observer = unsafe {
        NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
            Some(objc2_app_kit::NSApplicationDidBecomeActiveNotification),
            None,
            None,
            &block,
        )
    };
    // Kept for the life of the process: removing the observer would end
    // the watching, and nothing here ends before the process does.
    std::mem::forget(observer);
    Support::Yes
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A press on the Dock icon is reported as the tray's Open: the one
    /// thing the shell needs to hear to bring the window up.
    #[test]
    fn a_dock_press_reports_open() {
        let _serial = crate::tray::SERIAL
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let _ = crate::tray::drain_for_test();
        let app = objc2_foundation::NSObject::new();
        let answered = should_handle_reopen(
            &app,
            sel!(applicationShouldHandleReopen:hasVisibleWindows:),
            &app,
            Bool::NO,
        );
        assert!(answered.as_bool(), "and AppKit is left to activate as well");
        assert_eq!(crate::tray::drain_for_test(), vec![TrayAction::Open]);
    }

    /// Off the main thread -- every test -- the Dock is not touched, and
    /// the reason is said.
    #[test]
    fn watching_off_the_main_thread_refuses() {
        assert!(!watch().is_yes());
    }
}
