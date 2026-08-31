//! A navigation stack, and no business logic whatsoever.
//!
//! The same type serves both layers of sigil's navigation: the shell's one
//! global history across apps, and each column's private history. It knows how
//! to push, pop and replay; it knows nothing about what a route *is* or what it
//! owns. Freeing whatever a discarded route held is the caller's job.
//!
//! That division is the point — a stack that understood routes would have to
//! understand every app's routes.
//!
//! # Who frees what
//!
//! A route in sigil may own a live SIP-12 session, so leaking one means the
//! exchange keeps carrying a session nobody will close and the peer keeps
//! sending into it. But going *back* does not destroy a route: it is still
//! reachable by going forward, and freeing it there would break the replay.
//!
//! So a route dies at exactly one moment — when a new push discards the
//! forward branch it was sitting in. Every method that can do that returns the
//! routes it dropped, as [`Discarded`], which is `#[must_use]`. Ignoring one is
//! a compiler warning rather than a session leak found in production.

/// Routes that just became unreachable, for the caller to dispose of.
///
/// Empty most of the time. `#[must_use]` regardless: the cost of being reminded
/// about an empty one is a `let _ =`, and the cost of forgetting a full one is
/// a session the exchange carries forever.
#[must_use = "these routes are unreachable now; dispose of what they own"]
#[derive(Debug, PartialEq, Eq)]
pub struct Discarded<R>(pub Vec<R>);

impl<R> Discarded<R> {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &R> {
        self.0.iter()
    }
}

impl<R> IntoIterator for Discarded<R> {
    type Item = R;
    type IntoIter = std::vec::IntoIter<R>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

/// A back stack with forward history.
#[derive(Clone, Debug)]
pub struct NavStack<R> {
    routes: Vec<R>,
    forward: Vec<R>,
}

impl<R> NavStack<R> {
    /// A stack showing `root`.
    ///
    /// There is always at least one route. A stack that can be popped to
    /// nothing has no state to draw, and every caller would need a branch for
    /// the empty case that only ever fires as a bug.
    pub fn new(root: R) -> Self {
        Self {
            routes: vec![root],
            forward: Vec::new(),
        }
    }

    /// What is on screen.
    pub fn top(&self) -> &R {
        self.routes.last().expect("a NavStack is never empty")
    }

    pub fn top_mut(&mut self) -> &mut R {
        self.routes.last_mut().expect("a NavStack is never empty")
    }

    pub fn routes(&self) -> &[R] {
        &self.routes
    }

    pub fn depth(&self) -> usize {
        self.routes.len()
    }

    pub fn can_go_back(&self) -> bool {
        self.routes.len() > 1
    }

    pub fn can_go_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    /// Go to `route`, stacking it on top.
    ///
    /// Discards the forward history, as a browser does: having gone back and
    /// then somewhere new, the branch not taken is unreachable, and keeping it
    /// would make "forward" mean something nobody can predict. Those discarded
    /// routes are what comes back.
    pub fn push(&mut self, route: R) -> Discarded<R> {
        let dropped = std::mem::take(&mut self.forward);
        self.routes.push(route);
        Discarded(dropped)
    }

    /// Replace what is on screen, keeping the history beneath it.
    ///
    /// The replaced route is gone for good — unlike [`go_back`](Self::go_back),
    /// nothing can return to it — so it is discarded along with any forward
    /// branch.
    pub fn replace(&mut self, route: R) -> Discarded<R> {
        let mut dropped = std::mem::take(&mut self.forward);
        dropped.push(self.routes.pop().expect("a NavStack is never empty"));
        self.routes.push(route);
        Discarded(dropped)
    }

    /// Go back one, if there is anywhere to go.
    ///
    /// Discards nothing: the route moves into forward history and is still
    /// reachable. Returns whether it moved, so a caller can tell a no-op from a
    /// navigation without checking [`can_go_back`](Self::can_go_back) twice.
    pub fn go_back(&mut self) -> bool {
        if !self.can_go_back() {
            return false;
        }
        let route = self.routes.pop().expect("checked non-empty above");
        self.forward.push(route);
        true
    }

    /// Replay one step of forward history.
    pub fn go_forward(&mut self) -> bool {
        match self.forward.pop() {
            Some(route) => {
                self.routes.push(route);
                true
            }
            None => false,
        }
    }

    /// Pop back to the root in one step, for "close this thread and take me
    /// home". Everything above the root stays reachable by going forward.
    pub fn go_to_root(&mut self) -> bool {
        if !self.can_go_back() {
            return false;
        }
        while self.can_go_back() {
            self.go_back();
        }
        true
    }

    /// Drop the whole stack, handing back every route. Used when a column is
    /// closed: at that point even the forward branch is unreachable.
    pub fn close(self) -> Discarded<R> {
        let mut all = self.routes;
        all.extend(self.forward);
        Discarded(all)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stack() -> NavStack<&'static str> {
        NavStack::new("root")
    }

    #[test]
    fn a_new_stack_shows_its_root_and_cannot_go_anywhere() {
        let s = stack();
        assert_eq!(*s.top(), "root");
        assert!(!s.can_go_back());
        assert!(!s.can_go_forward());
    }

    #[test]
    fn going_back_discards_nothing_because_forward_can_still_reach_it() {
        let mut s = stack();
        let d = s.push("thread");
        assert!(d.is_empty());
        assert!(s.go_back());
        assert_eq!(*s.top(), "root");
        assert!(s.can_go_forward());
        assert!(s.go_forward());
        assert_eq!(*s.top(), "thread");
    }

    /// The moment a route actually dies, and the reason `Discarded` exists.
    #[test]
    fn pushing_after_going_back_discards_the_branch_not_taken() {
        let mut s = stack();
        let _ = s.push("call");
        assert!(s.go_back());
        let dropped = s.push("chat");
        assert_eq!(
            dropped.0,
            vec!["call"],
            "the abandoned branch comes back to be freed"
        );
        assert!(!s.can_go_forward());
    }

    #[test]
    fn replacing_discards_the_route_it_replaced() {
        let mut s = stack();
        let _ = s.push("first");
        let dropped = s.replace("second");
        assert_eq!(dropped.0, vec!["first"]);
        assert_eq!(*s.top(), "second");
        assert_eq!(s.depth(), 2, "the history beneath is kept");
    }

    #[test]
    fn replacing_also_discards_an_abandoned_forward_branch() {
        let mut s = stack();
        let _ = s.push("a");
        let _ = s.push("b");
        assert!(s.go_back()); // "b" is now only reachable forward
        let dropped = s.replace("c");
        let mut got = dropped.0;
        got.sort_unstable();
        assert_eq!(
            got,
            vec!["a", "b"],
            "both the replaced route and the branch"
        );
    }

    #[test]
    fn closing_hands_back_every_route_including_the_forward_branch() {
        let mut s = stack();
        let _ = s.push("a");
        let _ = s.push("b");
        assert!(s.go_back());
        let mut all = s.close().0;
        all.sort_unstable();
        assert_eq!(all, vec!["a", "b", "root"]);
    }

    #[test]
    fn going_to_root_keeps_the_way_back() {
        let mut s = stack();
        let _ = s.push("a");
        let _ = s.push("b");
        assert!(s.go_to_root());
        assert_eq!(*s.top(), "root");
        assert!(s.can_go_forward());
    }
}
