//! How long since anybody touched this machine.
//!
//! For saying "away": five minutes without a keypress or a click anywhere,
//! not only in sigil's window -- somebody typing in another program is not
//! away, and only the desktop knows that. macOS answers it from the event
//! system; the others have nothing sigil can ask, and the shell falls back
//! to what it can see itself.

/// Seconds since the last keyboard or pointer event on this desktop, or
/// `None` where the desktop cannot say.
pub fn seconds_since_input() -> Option<f64> {
    #[cfg(target_os = "macos")]
    {
        use objc2_core_graphics::{CGEventSource, CGEventSourceStateID, CGEventType};
        // `kCGAnyInputEventType` is `~0`, which the bindings do not name.
        let any_input = CGEventType(u32::MAX);
        let secs = CGEventSource::seconds_since_last_event_type(
            CGEventSourceStateID::CombinedSessionState,
            any_input,
        );
        // A negative or NaN answer would be the API misbehaving; treat it
        // as not knowing rather than as somebody being there.
        (secs.is_finite() && secs >= 0.0).then_some(secs)
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Where the desktop answers, it answers with a length of time.
    #[cfg(target_os = "macos")]
    #[test]
    fn macos_says_how_long() {
        let secs = seconds_since_input().expect("macOS can always say");
        assert!(secs >= 0.0);
    }

    /// Elsewhere it does not, and says so rather than guessing.
    #[cfg(not(target_os = "macos"))]
    #[test]
    fn elsewhere_it_cannot_say() {
        assert_eq!(seconds_since_input(), None);
    }
}
