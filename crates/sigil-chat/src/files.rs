//! Choosing a file to attach, or where to save one.
//!
//! On a desktop this is the native dialog, which blocks until it is answered
//! -- fine there, since it is the direct answer to a click and the session
//! runs on its own task regardless. A phone has no such call: choosing is an
//! activity the platform runs, and the answer arrives later, on a thread of
//! its own. So the shape here is the phone's, and the desktop answers it at
//! once: a [`Pick`] the interface asks each pass whether it has been
//! answered, and a [`Chooser`] a host installs to do the asking.
//!
//! **Nothing here is silently inert.** A phone build with no chooser
//! installed answers "nothing chosen" and says so in the log, rather than
//! hanging a pick forever.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

/// A choice being made. Cloneable, so the half that answers and the half
/// that asks can be on different threads.
#[derive(Clone)]
pub struct Pick(Arc<Mutex<State>>);

enum State {
    Pending,
    Answered(Option<Vec<PathBuf>>),
    Taken,
}

impl Pick {
    /// A pick already answered: the desktop's, where the dialog blocked.
    pub fn answered(paths: Option<Vec<PathBuf>>) -> Pick {
        Pick(Arc::new(Mutex::new(State::Answered(paths))))
    }

    /// A pick still being made, and the means to answer it.
    pub fn pending() -> (Pick, Answer) {
        let pick = Pick(Arc::new(Mutex::new(State::Pending)));
        (pick.clone(), Answer(pick.clone()))
    }

    /// `None` while the choice is still being made. `Some(None)` once it was
    /// made and nothing was chosen; `Some(Some(paths))` once, with what was.
    /// After that, `None` for ever: a choice is acted on once.
    pub fn take(&self) -> Option<Option<Vec<PathBuf>>> {
        let mut state = self.0.lock().unwrap_or_else(|e| e.into_inner());
        match std::mem::replace(&mut *state, State::Taken) {
            State::Pending => {
                *state = State::Pending;
                None
            }
            State::Answered(paths) => Some(paths),
            State::Taken => None,
        }
    }
}

/// The answering half of a pending [`Pick`].
pub struct Answer(Pick);

impl Answer {
    pub fn give(self, paths: Option<Vec<PathBuf>>) {
        let mut state = self.0.0.lock().unwrap_or_else(|e| e.into_inner());
        if matches!(*state, State::Pending) {
            *state = State::Answered(paths);
        }
    }
}

/// What asks the person. One per process, installed by the host.
pub trait Chooser: Send + Sync {
    /// Files to attach. Several at once, or none.
    fn pick_files(&self) -> Pick;
    /// Where to write a file that arrived under `name`. One path, or none.
    fn save_file(&self, name: &str) -> Pick;
}

static CHOOSER: OnceLock<Box<dyn Chooser>> = OnceLock::new();

/// Install the host's chooser. Once per process; a later call is ignored
/// and reported as such.
pub fn install(chooser: Box<dyn Chooser>) -> bool {
    CHOOSER.set(chooser).is_ok()
}

/// Ask for files to attach.
pub fn pick_files() -> Pick {
    chooser().pick_files()
}

/// Ask where to save a file.
pub fn save_file(name: &str) -> Pick {
    chooser().save_file(name)
}

fn chooser() -> &'static dyn Chooser {
    CHOOSER.get_or_init(|| Box::new(Default)).as_ref()
}

/// What answers when nothing was installed.
struct Default;

/// The native dialog, which blocks: the pick comes back answered.
#[cfg(not(target_os = "android"))]
impl Chooser for Default {
    fn pick_files(&self) -> Pick {
        Pick::answered(rfd::FileDialog::new().pick_files())
    }

    fn save_file(&self, name: &str) -> Pick {
        let mut dialog = rfd::FileDialog::new();
        if !name.is_empty() {
            dialog = dialog.set_file_name(name);
        }
        Pick::answered(dialog.save_file().map(|path| vec![path]))
    }
}

/// A phone with no chooser installed cannot ask, and must not look as if
/// it is asking. Answered at once with nothing, and said in the log.
#[cfg(target_os = "android")]
impl Chooser for Default {
    fn pick_files(&self) -> Pick {
        tracing::warn!("no file chooser installed on this phone; nothing can be attached");
        Pick::answered(None)
    }

    fn save_file(&self, _name: &str) -> Pick {
        tracing::warn!("no file chooser installed on this phone; nothing can be saved");
        Pick::answered(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Asked before it is answered, a pick says nothing; answered, it says
    /// so once and then never again -- a choice acted on twice would attach
    /// the same file twice.
    #[test]
    fn a_pick_is_taken_once() {
        let (pick, answer) = Pick::pending();
        assert!(pick.take().is_none());
        assert!(pick.take().is_none());
        answer.give(Some(vec![PathBuf::from("a")]));
        assert_eq!(pick.take(), Some(Some(vec![PathBuf::from("a")])));
        assert!(pick.take().is_none());
    }

    /// Nothing chosen is an answer, distinct from no answer yet.
    #[test]
    fn nothing_chosen_is_an_answer() {
        let (pick, answer) = Pick::pending();
        answer.give(None);
        assert_eq!(pick.take(), Some(None));
        let ready = Pick::answered(None);
        assert_eq!(ready.take(), Some(None));
    }
}
