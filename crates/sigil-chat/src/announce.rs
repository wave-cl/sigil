//! What tells the platform about rings, mentions and arrivals -- from a
//! frame, or from a session's wake when no frame is coming.
//!
//! # Why this is not a pass in `update`
//!
//! It was. Every pass walked the sessions for rings nobody had been told
//! about, and posted them. That is right on a desktop, where a window that
//! is not in front is still drawn whenever something changes. It is wrong on
//! a phone, where a window that is not in front is not drawn *at all*: the
//! surface is gone the moment somebody presses Home, `update` never runs
//! again until they come back, and a call that arrives in between reaches
//! the session, sits in its state, and is said to nobody. Seen on the
//! device: sigil in the background, connected -- the caller's exchange did
//! not refuse -- and silence. The one time a phone has to ring is the one
//! time it could not.
//!
//! So the deciding is here, behind a lock, and runs from two places: from a
//! frame, with what the frame knows (whether the window is in front); and
//! from a session's wake, when a frame was asked for and none came. The
//! second is what a phone in the background is.
//!
//! # The rule for "no frame came"
//!
//! A wake asks for a repaint first, then waits [`FRAME_GAP`]; if the frame
//! counter has not moved, nothing is drawing and the wake says it itself,
//! as if nobody were looking -- because nobody is. A desktop that is idle
//! also draws no frames, but it draws one within milliseconds of being asked,
//! so the wake there finds the counter moved and does nothing. This path is
//! only armed by a host that gives an off-frame notifier; a desktop gives
//! none.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use sigil::app::{AppAction, Notice, Notify, Sound, Target};
use sigil::quiet::Quiet;
use tokio::sync::watch;

use crate::session::{self, ChatState};
use crate::{At, arrivals_said, cross_key, mention_said, ring_said, target};

/// How long a wake waits for the frame it asked for before concluding that
/// nothing is drawing. A frame on any machine comes in a few milliseconds;
/// on a phone whose surface is gone it never comes.
pub const FRAME_GAP: Duration = Duration::from_millis(300);

pub struct Announcer {
    inner: Mutex<Inner>,
    /// Frames drawn, so a wake can tell whether one came after it asked.
    frames: AtomicU64,
    /// A wake's check is scheduled, so a burst of wakes schedules one.
    pending: AtomicBool,
    /// Wakes received, so a check knows whether more came while it waited.
    wakes: AtomicU64,
    /// The notifier for when no frame is coming. `None` on a desktop, where
    /// a frame always comes and the frame's own notifier is the one used.
    off_frame: Option<Arc<dyn Notify + Send + Sync>>,
}

#[derive(Default)]
struct Inner {
    /// Each session's state, read without the session: a watch receiver is
    /// the state's own channel and is cheap to hold.
    sessions: Vec<(At, watch::Receiver<ChatState>)>,
    quiet: Quiet,
    /// Rings and messages already said, so each is said once and not on
    /// every pass for as long as it stands.
    announced: HashSet<([u8; 32], u64)>,
    /// Rings announced and not yet withdrawn, each with where it was posted.
    ringing_out: HashMap<([u8; 32], u64), Target>,
    /// What the shell is asked for -- to come forward, for attention --
    /// which only a frame can hand over.
    wants: Vec<AppAction>,
}

impl Announcer {
    pub fn new(off_frame: Option<Arc<dyn Notify + Send + Sync>>) -> Arc<Announcer> {
        Arc::new(Announcer {
            inner: Mutex::new(Inner::default()),
            frames: AtomicU64::new(0),
            pending: AtomicBool::new(false),
            wakes: AtomicU64::new(0),
            off_frame,
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A frame is being drawn: what it knows, brought in.
    pub fn frame(&self, sessions: Vec<(At, watch::Receiver<ChatState>)>, quiet: &Quiet) {
        self.frames.fetch_add(1, Ordering::SeqCst);
        let mut inner = self.lock();
        inner.sessions = sessions;
        if inner.quiet != *quiet {
            inner.quiet = quiet.clone();
        }
    }

    /// A session started: watch it from now, frame or no frame.
    pub fn watch(&self, at: At, state: watch::Receiver<ChatState>) {
        let mut inner = self.lock();
        inner.sessions.retain(|(a, _)| *a != at);
        inner.sessions.push((at, state));
    }

    /// What the shell was asked for since last taken.
    pub fn take_wants(&self) -> Vec<AppAction> {
        std::mem::take(&mut self.lock().wants)
    }

    /// A session's state changed. The frame it asks for is asked for by
    /// the caller; this waits for it, and speaks only if it never comes.
    ///
    /// **One check at a time, and it repeats while wakes keep coming.** A
    /// burst of wakes schedules one check, not one thread each -- but a
    /// wake that arrives while a check is in flight is not dropped: the
    /// check goes round again, so the last change before silence is always
    /// looked at after the gap. The first version dropped it, and the ring
    /// it dropped was the one this exists for.
    pub fn woken(self: &Arc<Self>) {
        let Some(off_frame) = self.off_frame.clone() else {
            return;
        };
        self.wakes.fetch_add(1, Ordering::SeqCst);
        if self.pending.swap(true, Ordering::SeqCst) {
            return;
        }
        let me = self.clone();
        std::thread::spawn(move || {
            loop {
                let seen = me.wakes.load(Ordering::SeqCst);
                let mark = me.frames.load(Ordering::SeqCst);
                std::thread::sleep(FRAME_GAP);
                if me.frames.load(Ordering::SeqCst) == mark {
                    // Nobody is looking: the window is not being drawn.
                    me.run(&*off_frame, true);
                }
                if me.wakes.load(Ordering::SeqCst) == seen {
                    me.pending.store(false, Ordering::SeqCst);
                    // A wake between the load above and the store is the
                    // one race left; it schedules its own check.
                    if me.wakes.load(Ordering::SeqCst) == seen {
                        break;
                    }
                    if me.pending.swap(true, Ordering::SeqCst) {
                        break;
                    }
                }
            }
        });
    }

    /// Say what has not been said: rings first, then mentions, then the
    /// rest of what arrived. `unfocused` is whether anybody is looking.
    pub fn run(&self, notify: &dyn Notify, unfocused: bool) {
        let mut inner = self.lock();
        inner.rings(notify);
        inner.mentions(notify, unfocused);
        inner.arrivals(notify, unfocused);
    }

    // The deciding halves, over what a walk of the sessions found. Tests
    // call these with lists they built; the walks above call them with what
    // the sessions say.

    #[cfg(test)]
    pub(crate) fn rings_in(&self, notify: &dyn Notify, fresh: Vec<(String, Target)>) {
        self.lock().rings_in(notify, fresh);
    }

    #[cfg(test)]
    pub(crate) fn mentions_in(
        &self,
        notify: &dyn Notify,
        unfocused: bool,
        held: usize,
        found: Vec<(At, String, Vec<session::Mention>)>,
    ) {
        self.lock().mentions_in(notify, unfocused, held, found);
    }

    #[cfg(test)]
    pub(crate) fn arrivals_in(
        &self,
        notify: &dyn Notify,
        unfocused: bool,
        held: usize,
        found: Vec<(At, String, Vec<session::Arrival>)>,
    ) {
        self.lock().arrivals_in(notify, unfocused, held, found);
    }

    /// The rings walk alone, with the withdrawals: for the test that proves
    /// a ring that stopped is taken down.
    #[cfg(test)]
    pub(crate) fn rings_only(&self, notify: &dyn Notify) {
        self.lock().rings(notify);
    }

    #[cfg(test)]
    pub(crate) fn ringing_out_for_test(&self) -> HashMap<([u8; 32], u64), Target> {
        self.lock().ringing_out.clone()
    }

    #[cfg(test)]
    pub(crate) fn post_ring_for_test(&self, key: ([u8; 32], u64), to: Target) {
        self.lock().ringing_out.insert(key, to);
    }
}

impl Inner {
    /// **Which identity is being called.** Every session is walked, so a
    /// call arriving at one identity reaches somebody looking at another --
    /// and the notification then has to say which, or it names a caller, a
    /// conversation, and no way to tell where either of them is.
    fn rings(&mut self, notify: &dyn Notify) {
        let held = self.sessions.len();
        let mut fresh: Vec<(String, Target)> = Vec::new();
        for (at, session) in &self.sessions {
            let state = session.borrow();
            for ring in &state.ringing {
                if ring.mine || self.announced.contains(&(ring.channel, ring.seq)) {
                    continue;
                }
                self.announced.insert((ring.channel, ring.seq));
                self.ringing_out
                    .insert((ring.channel, ring.seq), target(at, ring.channel));
                let me = state.mine.label(&at.0);
                fresh.push((
                    ring_said(&ring.from, &ring.label, &me, held),
                    target(at, ring.channel),
                ));
            }
            // SIP-39: a call carried here from another exchange rings the
            // same way -- the phone has to make a sound and come forward for
            // it, or a ring nobody is looking at is a ring nobody hears. It
            // has no conversation, so its notification is keyed on the
            // bridge; pressing it brings the window up, which is where the
            // ring is drawn whatever else is on screen.
            if let Some(cross) = &state.cross_ring {
                let key = cross_key(cross.bridge);
                if !self.announced.contains(&(key, 0)) {
                    self.announced.insert((key, 0));
                    self.ringing_out.insert((key, 0), target(at, key));
                    let me = state.mine.label(&at.0);
                    // The caller first, by the name this client has for
                    // them, and where from after.
                    let who = state
                        .people
                        .get(&cross.caller)
                        .map(|p| p.label(&cross.caller))
                        .unwrap_or_else(|| sigil_ui::message::short(&cross.caller.to_string()));
                    let said = if held > 1 {
                        format!("{who}, from another exchange — to {me}")
                    } else {
                        format!("{who}, from another exchange")
                    };
                    fresh.push((said, target(at, key)));
                }
            }
        }
        self.rings_in(notify, fresh);
        self.withdraw_gone(notify);
    }

    /// The deciding half: rings nobody has been told about yet are said,
    /// with a sound, leading to the conversation ringing; and the window is
    /// asked for.
    fn rings_in(&mut self, notify: &dyn Notify, fresh: Vec<(String, Target)>) {
        // A muted conversation, or do-not-disturb: it rings on the screen,
        // and nowhere else.
        let fresh: Vec<(String, Target)> = fresh
            .into_iter()
            .filter(|(_, to)| !self.quiet.silenced(&to.exchange, &to.channel))
            .collect();
        if !fresh.is_empty() {
            // A call reaches somebody looking at something else: the window
            // comes forward, and where the desktop will not let it, the icon
            // asks until it is answered.
            self.wants.push(AppAction::Present);
            self.wants
                .push(AppAction::Attention(sigil::Attention::Critical));
        }
        for (said, to) in fresh {
            notify.notice(Notice {
                summary: "Incoming call",
                body: &said,
                target: Some(to),
                sound: Sound::Ring,
            });
        }
    }

    /// Take down the notification for a ring that has stopped ringing.
    ///
    /// **Nothing did this, on any platform.** The Android glue has had the
    /// call to withdraw a ring since rings existed and no Rust ever made it,
    /// so every ring posted on that phone stayed on the notification shade
    /// for good: ongoing, so it could not be swiped away, and still offering
    /// Answer for a call that had ended minutes before.
    ///
    /// A ring that is no longer in the session's list has been answered,
    /// declined, cancelled or missed. All four mean the same thing here.
    fn withdraw_gone(&mut self, notify: &dyn Notify) {
        if self.ringing_out.is_empty() {
            return;
        }
        let mut live: HashSet<([u8; 32], u64)> = HashSet::new();
        for (_, session) in &self.sessions {
            let state = session.borrow();
            live.extend(state.ringing.iter().map(|r| (r.channel, r.seq)));
            if let Some(c) = &state.cross_ring {
                live.insert((cross_key(c.bridge), 0));
            }
        }
        let gone: Vec<([u8; 32], u64)> = self
            .ringing_out
            .keys()
            .filter(|k| !live.contains(*k))
            .copied()
            .collect();
        for key in gone {
            if let Some(to) = self.ringing_out.remove(&key) {
                notify.withdraw(&to);
            }
        }
    }

    /// Say out loud that somebody mentioned us, when we are not looking.
    /// Told once per message, whether or not it is posted: a mention read on
    /// screen as it arrived is not owed a notification later.
    fn mentions(&mut self, notify: &dyn Notify, unfocused: bool) {
        let held = self.sessions.len();
        let mut found: Vec<(At, String, Vec<session::Mention>)> = Vec::new();
        for (at, session) in &self.sessions {
            let state = session.borrow();
            let mentions: Vec<session::Mention> = state
                .arrivals
                .iter()
                .filter(|a| a.mentions_me)
                .cloned()
                .collect();
            if mentions.is_empty() {
                continue;
            }
            let me = state.mine.label(&at.0);
            found.push((at.clone(), me, mentions));
        }
        self.mentions_in(notify, unfocused, held, found);
    }

    /// The deciding half, over what the sessions said: posted when the
    /// window is not in front or the conversation is not the one on screen,
    /// and once.
    fn mentions_in(
        &mut self,
        notify: &dyn Notify,
        unfocused: bool,
        held: usize,
        found: Vec<(At, String, Vec<session::Mention>)>,
    ) {
        for (at, me, mentions) in found {
            for m in mentions {
                if !self.announced.insert((m.channel, m.seq)) {
                    continue;
                }
                if self.quiet.silenced(&at.1, &m.channel) {
                    continue;
                }
                if unfocused || !m.in_open {
                    let (summary, body) = mention_said(&m, &me, held);
                    notify.notice(Notice {
                        summary: &summary,
                        body: &body,
                        target: Some(target(&at, m.channel)),
                        sound: Sound::Default,
                    });
                }
                // Worth noticing, not worth interrupting for: the icon
                // bounces once while the window is not in front.
                if unfocused {
                    self.wants
                        .push(AppAction::Attention(sigil::Attention::Informational));
                }
            }
        }
    }

    /// Say out loud that a message arrived, when we are not looking. After
    /// the mentions: a mention has its own words and is not said twice. Only
    /// while the window is not in front -- in front, a message in another
    /// conversation is what the list's count is for. Several arriving
    /// together in one conversation are one notification.
    fn arrivals(&mut self, notify: &dyn Notify, unfocused: bool) {
        let held = self.sessions.len();
        let mut found: Vec<(At, String, Vec<session::Arrival>)> = Vec::new();
        for (at, session) in &self.sessions {
            let state = session.borrow();
            if state.arrivals.is_empty() {
                continue;
            }
            let me = state.mine.label(&at.0);
            found.push((at.clone(), me, state.arrivals.clone()));
        }
        self.arrivals_in(notify, unfocused, held, found);
    }

    /// The deciding half. Told once per message whether or not it is posted:
    /// one read on screen as it arrived is not owed a notification later.
    fn arrivals_in(
        &mut self,
        notify: &dyn Notify,
        unfocused: bool,
        held: usize,
        found: Vec<(At, String, Vec<session::Arrival>)>,
    ) {
        for (at, me, arrivals) in found {
            let mut fresh: BTreeMap<[u8; 32], Vec<session::Arrival>> = Default::default();
            for a in arrivals {
                // Mentions are said with their own words, before this.
                if a.mentions_me || !self.announced.insert((a.channel, a.seq)) {
                    continue;
                }
                if unfocused && !self.quiet.silenced(&at.1, &a.channel) {
                    fresh.entry(a.channel).or_default().push(a);
                }
            }
            for (channel, together) in fresh {
                let (summary, body) = arrivals_said(&together, &me, held);
                notify.notice(Notice {
                    summary: &summary,
                    body: &body,
                    target: Some(target(&at, channel)),
                    sound: Sound::Default,
                });
            }
        }
    }
}
