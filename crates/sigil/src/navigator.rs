//! The shell's global history, and the queue apps use to ask for navigation.
//!
//! An app never touches the real stack. It pushes a [`NavRequest`] onto the
//! frame-local [`Navigator`], and the shell drains the queue after rendering.
//! Two things fall out of that:
//!
//! - **App switching is navigation.** The shell's `active` app is *derived*
//!   from the top of the stack rather than stored beside it, so back and
//!   forward cross app boundaries for nothing.
//! - **The shell stays ignorant.** A history entry carries an opaque
//!   `Rc<dyn Any>` route token it never inspects, only hands back to the app
//!   that pushed it.

use std::any::Any;
use std::rc::Rc;

/// Which app slot in the shell's roster an entry belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AppId(pub usize);

impl AppId {
    pub fn slot(self) -> usize {
        self.0
    }
}

/// One entry of the global history.
#[derive(Clone)]
pub struct NavEntry {
    pub app: AppId,
    /// Opaque, app-defined. The shell never looks inside; the owning app
    /// downcasts it back to its own route type when asked to draw it.
    ///
    /// `Rc` rather than `Box` because the shell clones entries while animating
    /// a transition, and a refcount bump is the right price for that.
    pub token: Rc<dyn Any>,
}

impl NavEntry {
    pub fn new<R: Any>(app: AppId, route: R) -> Self {
        Self {
            app,
            token: Rc::new(route),
        }
    }

    /// An entry that names an app and no particular view inside it — what a
    /// plain tab click pushes.
    pub fn app_only(app: AppId) -> Self {
        Self {
            app,
            token: Rc::new(()),
        }
    }
}

impl std::fmt::Debug for NavEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The token is deliberately opaque, so there is nothing honest to print
        // about it. Saying so beats printing a pointer.
        f.debug_struct("NavEntry")
            .field("app", &self.app)
            .field("token", &"<opaque>")
            .finish()
    }
}

/// A route an app raised about *itself*, before the shell has tagged it.
///
/// An app does not know its own slot in the roster — it should not have to —
/// so it enqueues an untagged token and the shell stamps the active `AppId` on
/// it while draining.
#[derive(Clone)]
pub struct ActiveEntry {
    pub token: Rc<dyn Any>,
}

impl ActiveEntry {
    pub fn tag(self, app: AppId) -> NavEntry {
        NavEntry {
            app,
            token: self.token,
        }
    }
}

/// What an app is asking the shell to do.
#[derive(Clone)]
pub enum NavRequest {
    /// Go to a view in a named app — used to cross from one app to another.
    Push(NavEntry),
    Replace(NavEntry),
    /// Go to a view in whichever app is asking.
    PushActive(ActiveEntry),
    ReplaceActive(ActiveEntry),
    Back,
    Forward,
}

/// A frame-local queue of navigation requests.
///
/// Drained by the shell after render, then reused. `take` hands the allocation
/// off rather than freeing and re-growing it every frame.
#[derive(Default)]
pub struct Navigator {
    requests: Vec<NavRequest>,
}

impl Navigator {
    /// Go to `route` inside the app that is asking.
    pub fn push_here<R: Any>(&mut self, route: R) {
        self.requests.push(NavRequest::PushActive(ActiveEntry {
            token: Rc::new(route),
        }));
    }

    /// Replace the current view inside the app that is asking.
    pub fn replace_here<R: Any>(&mut self, route: R) {
        self.requests.push(NavRequest::ReplaceActive(ActiveEntry {
            token: Rc::new(route),
        }));
    }

    /// Go to `route` in another app — opening a conversation from a call, say.
    pub fn push_in<R: Any>(&mut self, app: AppId, route: R) {
        self.requests
            .push(NavRequest::Push(NavEntry::new(app, route)));
    }

    /// Switch to an app without naming a view inside it.
    pub fn switch_to(&mut self, app: AppId) {
        self.requests
            .push(NavRequest::Push(NavEntry::app_only(app)));
    }

    pub fn back(&mut self) {
        self.requests.push(NavRequest::Back);
    }

    pub fn forward(&mut self) {
        self.requests.push(NavRequest::Forward);
    }

    pub fn is_empty(&self) -> bool {
        self.requests.is_empty()
    }

    /// Take everything queued this frame. The shell calls this; apps do not.
    pub fn take(&mut self) -> Vec<NavRequest> {
        std::mem::take(&mut self.requests)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_drain_in_the_order_they_were_raised() {
        let mut n = Navigator::default();
        n.push_here(1u32);
        n.back();
        n.switch_to(AppId(2));
        let got = n.take();
        assert_eq!(got.len(), 3);
        assert!(matches!(got[0], NavRequest::PushActive(_)));
        assert!(matches!(got[1], NavRequest::Back));
        assert!(matches!(got[2], NavRequest::Push(_)));
        assert!(
            n.is_empty(),
            "taking leaves the queue empty for the next frame"
        );
    }

    #[test]
    fn an_untagged_route_becomes_the_active_apps_once_stamped() {
        let mut n = Navigator::default();
        n.push_here("thread");
        let Some(NavRequest::PushActive(e)) = n.take().into_iter().next() else {
            panic!("expected an untagged active push");
        };
        let entry = e.tag(AppId(7));
        assert_eq!(entry.app, AppId(7));
        assert_eq!(entry.token.downcast_ref::<&str>(), Some(&"thread"));
    }

    /// The shell hands a token back to its owning app, which downcasts it. An
    /// app must survive being handed something it does not recognise -- another
    /// app's route type, or the `()` of a plain tab switch -- rather than
    /// panicking on it.
    #[test]
    fn an_unrecognised_token_downcasts_to_none_rather_than_panicking() {
        let entry = NavEntry::app_only(AppId(0));
        assert_eq!(entry.token.downcast_ref::<u32>(), None);
        let other = NavEntry::new(AppId(1), 5u32);
        assert_eq!(other.token.downcast_ref::<String>(), None);
        assert_eq!(other.token.downcast_ref::<u32>(), Some(&5));
    }
}
