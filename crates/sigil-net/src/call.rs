//! Running a call on a task, and telling the interface what it is doing.
//!
//! `sqex_voice::engine` owns the call loop. This owns the *relationship*
//! between that loop and something with a window: it runs the loop on a task,
//! folds what the loop reports into a snapshot the UI can draw, and wakes the
//! UI only when there is something new to draw.
//!
//! # Two channels, on purpose
//!
//! State and narrative are kept apart, because they have different lifetimes
//! and merging them loses one of them:
//!
//! - **[`CallState`]** is what is true *now* — the phase, the session, the last
//!   statistics line. It goes through a `watch`, where only the latest value
//!   matters and a slow reader misses nothing important.
//! - **Events** are what *happened* — somebody joined, a frame would not open,
//!   the peer has said nothing at all. They go through an unbounded queue,
//!   because dropping one loses the only notice of it.
//!
//! sqex-chat learned this the hard way in its own interface: a note about an
//! action and a status about a state shared one field, so every confirmation it
//! ever printed was on screen for less than a second.

use std::sync::Arc;

use crate::held::Held;
use sqex_proto::room::RoomId;
use sqex_voice::engine::{self, CallOpts, Endpoint, Event, PeerStatus, Report};
// `Signer` for `public()`: a call placed on a connection somebody else opened
// still has to refuse being placed to oneself, and that check needs our key.
use sqnr_core::{PubKey, Signer, SoftwareSigner};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

/// Where a call has got to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    /// Not started.
    #[default]
    Idle,
    /// Finding the exchange, then dialling it. Discovery may involve a DNSSEC
    /// lookup, so this is not always instant.
    Connecting,
    /// Connected, waiting for the peer to name us in return. Consent is
    /// mutual, so this can last as long as the other person takes.
    Waiting,
    /// Media is flowing.
    Live,
    /// Over. `Ended(None)` is a call that finished; `Ended(Some(_))` is one
    /// that failed, and the string is worth showing.
    Ended,
}

/// How a call's media travels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Path {
    /// Straight between the two parties, after the exchange introduced them
    /// (SIP-25).
    Direct,
    /// Through the exchange (SIP-12). Either nobody asked for an
    /// introduction, or one was made and led nowhere; `CallState::why` says
    /// which.
    Relayed,
}

/// Everything the interface needs to draw a call, and nothing it does not.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CallState {
    pub phase: Phase,
    /// How the media travels, once that is settled. `None` while it is not
    /// -- during the introduction, or for a call that never asked.
    pub path: Option<Path>,
    /// Why the call is relayed when a direct connection was wanted: the
    /// peer did not ask, or the punch failed and this says how.
    pub why: Option<String>,
    /// Who the exchange thinks we are. Shown in full somewhere reachable: a
    /// name is an assertion, a key is not (SIP-21).
    pub me: Option<PubKey>,
    /// The peer we are calling, if this is a two-party call.
    pub peer: Option<PubKey>,
    pub session: Option<u64>,
    /// The most recent statistics line, replaced each second.
    pub stats: Option<String>,
    /// The closing summary, once there is one. Separate from `stats` because
    /// it survives the call ending, and is the one number worth having
    /// afterwards.
    pub final_stats: Option<String>,
    /// Why the call ended badly, if it did.
    pub trouble: Option<String>,
    /// Nothing has arrived from the peer at all. A distinct flag rather than a
    /// log line, because the interface should be able to say so loudly.
    pub deaf: bool,
    /// Who else is in the room, sorted and stable, with who is speaking.
    /// Empty for a two-party call.
    pub present: Vec<PeerStatus>,
    /// Members whose session is not up yet. They are in the room and cannot be
    /// heard, which is a different thing from not being there.
    pub connecting: usize,
    /// The room this is, if it is a room. Held so the interface can offer the
    /// secret again — it is the only way anyone else gets in.
    pub room: Option<RoomId>,
}

impl CallState {
    pub fn is_over(&self) -> bool {
        self.phase == Phase::Ended
    }
}

/// Folds what the engine reports into the snapshot and the event queue.
struct Bridge {
    state: watch::Sender<CallState>,
    events: mpsc::UnboundedSender<Event>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl Report for Bridge {
    fn event(&mut self, event: Event) {
        // Every step of a call, in the log: a call that ended eight seconds
        // in on a phone was ending for a reason nothing had written down.
        // Stats and presence are a line a second each, and are not.
        if !matches!(event, Event::Stats(_) | Event::Present { .. }) {
            tracing::info!(?event, "call");
        }
        // Update the snapshot first, so a wake never arrives before the state
        // it is waking somebody up to look at.
        self.state.send_modify(|s| match &event {
            Event::Identity(me) => {
                s.me = Some(*me);
                s.phase = Phase::Connecting;
            }
            Event::Waiting { peer } => {
                s.peer = Some(*peer);
                s.phase = Phase::Waiting;
            }
            Event::SessionUp { id, .. } | Event::Reflecting { id } => {
                s.session = Some(*id);
                s.phase = Phase::Live;
            }
            Event::RoomJoined { me, .. } => {
                s.me = Some(*me);
                s.phase = Phase::Live;
            }
            Event::Stats(line) => s.stats = Some(line.clone()),
            Event::FinalStats(line) => {
                s.stats = Some(line.clone());
                s.final_stats = Some(line.clone());
            }
            Event::Deaf => s.deaf = true,
            Event::Present { peers, connecting } => {
                s.present = peers.clone();
                s.connecting = *connecting;
            }
            Event::Direct { .. } => s.path = Some(Path::Direct),
            Event::Relayed { why } => {
                s.path = Some(Path::Relayed);
                s.why = Some(why.clone());
            }
            // The rest are narrative: they say what happened, not what is true.
            // `Moved` is SIP-40: the exchange's pinned key changed hands and
            // the pin followed a signed handover. Like `Pinned`, it is said
            // once through `describe()` and holds no state -- the new key is
            // already what every later connection checks against.
            Event::Pinned { .. }
            | Event::Moved { .. }
            | Event::StillWaiting { .. }
            | Event::Roster(_)
            | Event::Draining
            | Event::BadFrame { .. }
            | Event::Reflected(_)
            | Event::CallerGone { .. }
            // SIP-39's cross-exchange decline: the callee's exchange tells the
            // caller rather than leaving them to time out. sigil does not take
            // that path yet -- it rings over the SIP-5 mailbox and declines by
            // simply not answering -- so this only ever arrives as something to
            // say, not as a state to hold.
            | Event::Declined { .. }
            // The path closing under the call ends it; the task's epilogue
            // records the ending, and this only says why.
            | Event::Closed { .. }
            | Event::Device(_) => {}
        });
        // A closed receiver means the interface has gone; the call carries on
        // regardless, because hanging up is a decision and not a side effect of
        // nobody watching.
        let _ = self.events.send(event);
        (self.wake)();
    }
}

/// How long a hung-up call has to drain and post its `/session/close`
/// before it is dropped where it stands.
///
/// The engine's own drain is 500 ms plus the jitter depth; the rest is room
/// for one round trip to the exchange on a path that is already unwell.
const HANGUP_GRACE: std::time::Duration = std::time::Duration::from_secs(3);

/// A running call.
///
/// Dropping this does **not** end the call — the task owns it. Use
/// [`hang_up`](CallHandle::hang_up), so that ending a call is always something
/// somebody decided.
pub struct CallHandle {
    state: watch::Receiver<CallState>,
    events: mpsc::UnboundedReceiver<Event>,
    task: JoinHandle<Result<(), String>>,
    /// The same sender the task holds, so that **hanging up can report the
    /// ending itself**.
    ///
    /// The task reports one when its work finishes, and aborting drops the
    /// task at whatever it was awaiting — so the epilogue never runs and the
    /// snapshot keeps saying `Live` for ever. Which is precisely what
    /// happened: pressing Leave ended the call and left its screen up, with no
    /// way back.
    ending: watch::Sender<CallState>,
    /// Asks the engine to end the call the way its source running out does:
    /// drain what is in flight, then post `/session/close`.
    ///
    /// **Aborting the task skips that close**, because it cancels the task
    /// at its next await. On a dialled connection the connection dropping
    /// is itself the signal, so it did not matter. On a **borrowed** one
    /// -- a SIP-39 call across exchanges rides the chat session, which is
    /// not ours to drop -- the far side is told nothing and goes on
    /// sending into a session this end has forgotten. Seen live: a phone
    /// still streaming 24 minutes after the desktop hung up, its frames
    /// counted and discarded as `stale` at the next call.
    stop: watch::Sender<bool>,
    wake: Arc<dyn Fn() + Send + Sync>,
}

impl CallHandle {
    /// The current snapshot. Cheap: a clone of a small struct.
    pub fn state(&self) -> CallState {
        self.state.borrow().clone()
    }

    /// Everything reported since the last drain, oldest first.
    pub fn drain(&mut self) -> Vec<Event> {
        let mut out = Vec::new();
        while let Ok(event) = self.events.try_recv() {
            out.push(event);
        }
        out
    }

    /// End the call.
    ///
    /// Aborting the task drops the engine's future, which drops the session and
    /// the audio devices with it. The `/session/close` the engine would have
    /// posted does not get sent — the exchange times the session out instead,
    /// which is the same trade the CLI's signal handler already makes and is
    /// documented there.
    pub fn hang_up(&self) {
        // The last second's statistics, before the task that would have
        // summed them up is gone: a call ended from this side leaves no
        // closing summary, and the last line is what there is to read of
        // how it went.
        let last = self.ending.borrow().stats.clone();
        tracing::info!(stats = ?last, "hung up");
        // **Asked, not killed.** The engine drains what is in flight and
        // posts `/session/close`, which is the only thing that tells a peer
        // on a borrowed connection that the call is over.
        let _ = self.stop.send(true);
        // With a backstop, because a task that cannot reach its close would
        // otherwise hold the microphone for the life of the window and the
        // next call would find it taken. The grace is the engine's own
        // drain plus room for the post; past that, the old behaviour.
        match tokio::runtime::Handle::try_current() {
            Ok(rt) => {
                let abort = self.task.abort_handle();
                rt.spawn(async move {
                    tokio::time::sleep(HANGUP_GRACE).await;
                    abort.abort();
                });
            }
            // Nowhere to wait: end it now rather than leave it running.
            Err(_) => self.task.abort(),
        }
        // Said here rather than left to the task, which will not run again.
        self.ending.send_modify(|s| {
            s.phase = Phase::Ended;
            // Nobody can hear you once you have gone, so a roster left on
            // screen would be a list of people who cannot.
            s.present.clear();
            s.connecting = 0;
            // Not `trouble`: hanging up is a decision, and reporting it as
            // something that went wrong would put an error on screen for
            // having done what was asked.
        });
        (self.wake)();
    }

    /// Wait for the call to finish on its own.
    pub async fn finished(self) -> Result<(), String> {
        match self.task.await {
            Ok(result) => result,
            Err(e) if e.is_cancelled() => Ok(()),
            Err(e) => Err(format!("the call task failed: {e}")),
        }
    }

    /// Await the next change to the snapshot. For tests and for anything that
    /// would rather wait than poll.
    pub async fn changed(&mut self) -> Result<(), String> {
        self.state
            .changed()
            .await
            .map_err(|_| "the call ended".to_string())
    }
}

/// Where to dial: an exchange already resolved, or the layers to resolve one
/// from.
///
/// Discovery is a DNSSEC lookup and belongs on the task with everything else,
/// not in front of the interface — a window must not stop painting while a
/// name is looked up.
pub enum Dial {
    /// An exchange already known, as tests and a settings pane both have.
    At(Endpoint),
    /// Resolve one first. See [`crate::discovery::layers`].
    ///
    /// A `Vec` rather than a fixed-size array: the layer count varies now that
    /// the identity's SIP-38 handle contributes one only when there is an
    /// identity with a handle.
    Discover(Vec<sqex_discovery::Layer>),
    /// **Do not dial at all**: this identity is already connected to the
    /// exchange, and the call goes on the connection it holds.
    ///
    /// A chat session has one open for as long as the window is, so pressing
    /// call need not wait for a handshake — and, for as long as the call lasts,
    /// the exchange has one connection to fan its datagrams to rather than two.
    /// It writes a relayed datagram to *every* connection an identity holds, so
    /// the second one is not idle while a call is up: it carries a duplicate of
    /// every audio frame, which nothing reads.
    ///
    /// Whoever lends the connection gives up reading datagrams on it for the
    /// duration; a call is the only thing here that reads them, and a chat
    /// client never does.
    ///
    /// A [`Held`] rather than a connection, because a connection is not a
    /// stable thing to be handed: the session that owns it redials. A call
    /// takes whatever is live when it starts, which is the right reading for a
    /// call — one that outlived its connection would have nothing to carry
    /// audio on anyway.
    On(Held),
    /// SIP-85: reach `target` through `home`, which carries the connection
    /// so the target sees the home's address and this identity's own key.
    /// Only a chat session takes this: it opens the tunnel and owns it, and
    /// a call at that exchange rides the session's connection (`On`), never
    /// a tunnel of its own. `home` and `target` are `At` or `Discover`;
    /// `target_domain` is what the home is asked to find the target by.
    Via {
        home: Box<Dial>,
        target: Box<Dial>,
        target_domain: String,
    },
}

impl From<Held> for Dial {
    fn from(h: Held) -> Self {
        Dial::On(h)
    }
}

impl Dial {
    /// The borrowed connection if there is a live one, and where to dial if not.
    ///
    /// The rule every part of the window that starts a call follows, in one
    /// place because it has three callers and the order of its two halves is
    /// the whole of it:
    ///
    /// - **Borrow first.** One identity should reach one exchange over one
    ///   connection; a second costs a handshake now and a duplicate of every
    ///   audio frame afterwards. A live connection is enough on its own — an
    ///   exchange that nothing is configured for is still an exchange this
    ///   identity is connected to.
    /// - **Dial only when there is nothing to borrow**, and only when something
    ///   says where. `None` means neither: no live connection and no exchange
    ///   configured, which is a call that cannot be placed and an interface
    ///   that should say so rather than start a task to fail.
    ///
    /// A slot that exists but is empty is *not* borrowable here. That is the
    /// difference between a call and the administrative console, which waits
    /// for one: a call is placed at the moment somebody asks for it, and asking
    /// somebody to wait for a handshake we could have made ourselves is worse
    /// than making it.
    pub fn borrowed_or(held: Option<Held>, layers: Vec<sqex_discovery::Layer>) -> Option<Dial> {
        if let Some(held) = held.filter(|h| h.is_live()) {
            return Some(Dial::On(held));
        }
        crate::discovery::any_configured(&layers).then_some(Dial::Discover(layers))
    }
}

/// SIP-85: a call never opens a tunnel of its own. At an exchange reached
/// through the home it rides the chat session's connection, which is the
/// tunnelled one; with that connection down there is nothing to ride.
pub const NO_CALL_TUNNEL: &str =
    "a call at an exchange reached through your home needs the chat connection, which is down";

impl From<Endpoint> for Dial {
    fn from(e: Endpoint) -> Self {
        Dial::At(e)
    }
}

impl From<Vec<sqex_discovery::Layer>> for Dial {
    fn from(l: Vec<sqex_discovery::Layer>) -> Self {
        Dial::Discover(l)
    }
}

/// Place a call, on a task of its own.
///
/// `wake` is called whenever anything changes — pass `egui`'s
/// `Context::request_repaint`. Everything here is event-driven precisely so
/// that a silent call costs nothing to display: an interface that redrew at
/// sixty frames a second through an hour of quiet would be the largest single
/// consumer of power in the application.
pub fn spawn_call(
    dial: impl Into<Dial>,
    signer: SoftwareSigner,
    peer: PubKey,
    wait: u64,
    opts: CallOpts,
    wake: impl Fn() + Send + Sync + 'static,
) -> CallHandle {
    let dial = dial.into();
    let (state_tx, state_rx) = watch::channel(CallState {
        phase: Phase::Connecting,
        peer: Some(peer),
        ..CallState::default()
    });
    let (events_tx, events_rx) = mpsc::unbounded_channel();
    // Asked-to-stop, so hanging up drains and closes rather than dropping
    // the task at its next await; see `CallHandle::stop`.
    let (stop_tx, stop_rx) = watch::channel(false);
    let opts = CallOpts {
        stop: Some(stop_rx),
        ..opts
    };
    let wake = Arc::new(wake);

    let ending = state_tx.clone();
    let ending_wake = wake.clone();
    let hanging_up = state_tx.clone();
    let hanging_up_wake = wake.clone();
    let task = tokio::spawn(async move {
        let mut bridge = Bridge {
            state: state_tx,
            events: events_tx,
            wake,
        };
        let result = async {
            if PubKey::new(signer.public()) == peer {
                return Err("a session needs two identities".to_string());
            }
            let mut client = match dial {
                Dial::On(held) => {
                    let (client, _) = held.now().ok_or("not connected to the exchange")?;
                    engine::adopt(client, &signer, &mut bridge)?
                }
                Dial::At(e) => engine::dial(e, &signer, peer, &mut bridge).await?,
                Dial::Discover(layers) => {
                    let e = engine::resolve(&layers[..], &mut bridge).await?;
                    engine::dial(e, &signer, peer, &mut bridge).await?
                }
                Dial::Via { .. } => return Err(NO_CALL_TUNNEL.to_string()),
            };
            // Ring before waiting, so the other end has a reason to answer.
            // Best effort on purpose: a ring that does not arrive costs a call
            // that has to be arranged another way, and refusing to place the
            // call at all would be worse. Somebody who was already expecting
            // this does not need the ring, and their session opens regardless.
            if let Err(e) = sqex_voice::ring::ring(&mut client, peer).await {
                bridge.event(Event::BadFrame {
                    seq: 0,
                    why: format!("could not ring: {e}"),
                });
            }
            let (session, id) =
                engine::rendezvous(&mut client, &signer, peer, wait, &mut bridge).await?;
            engine::call(client, session, id, opts, &mut bridge).await
        }
        .await;

        // Whatever happened, the interface must be told the call is over --
        // otherwise a failed call sits on screen looking like a connecting one
        // forever.
        match &result {
            Ok(_) => tracing::info!("call over"),
            Err(e) => tracing::warn!("call over with trouble: {e}"),
        }
        ending.send_modify(|s| {
            s.phase = Phase::Ended;
            if let Err(e) = &result {
                s.trouble = Some(e.clone());
            }
        });
        (ending_wake)();
        result
    });

    CallHandle {
        stop: stop_tx,
        state: state_rx,
        events: events_rx,
        task,
        ending: hanging_up,
        wake: hanging_up_wake,
    }
}

/// **SIP-39: call somebody at another exchange, by name.**
///
/// [`spawn_call`] dials a key at *this* exchange and rendezvous with it
/// there. A person whose account lives somewhere else has no session here to
/// meet in, so the call is placed at this exchange for `target` --
/// `name@domain` -- and this exchange carries it to theirs, which rings
/// them. The media never touches either: the session key is derived over the
/// two identities and the two ephemerals, and no exchange on the path, near
/// or far, ever holds it.
///
/// `target` is a string rather than a key on purpose. A key is a thing you
/// already have; the point of calling across exchanges is reaching somebody
/// you know by name, and it is their home that turns the name into a key.
/// Nothing here learns it -- the handshake does, inside the session -- so
/// `CallState::peer` stays empty and the interface shows what was typed.
pub fn spawn_cross_call(
    dial: impl Into<Dial>,
    signer: SoftwareSigner,
    target: String,
    wait: u64,
    opts: CallOpts,
    wake: impl Fn() + Send + Sync + 'static,
) -> CallHandle {
    let dial = dial.into();
    let (state_tx, state_rx) = watch::channel(CallState {
        phase: Phase::Connecting,
        ..CallState::default()
    });
    let (events_tx, events_rx) = mpsc::unbounded_channel();
    // Asked-to-stop, so hanging up drains and closes rather than dropping
    // the task at its next await; see `CallHandle::stop`.
    let (stop_tx, stop_rx) = watch::channel(false);
    let opts = CallOpts {
        stop: Some(stop_rx),
        ..opts
    };
    let wake = Arc::new(wake);

    let ending = state_tx.clone();
    let ending_wake = wake.clone();
    let hanging_up = state_tx.clone();
    let hanging_up_wake = wake.clone();
    let task = tokio::spawn(async move {
        let mut bridge = Bridge {
            state: state_tx,
            events: events_tx,
            wake,
        };
        let result = async {
            // The endpoint, by the same three ways a direct call reaches
            // one. A borrowed connection is this identity's own session, so
            // the call is placed on the connection it already holds.
            let endpoint = match &dial {
                Dial::On(held) => {
                    let (_, endpoint) = held.now().ok_or("not connected to the exchange")?;
                    endpoint
                }
                Dial::At(e) => *e,
                Dial::Discover(layers) => engine::resolve(&layers[..], &mut bridge).await?,
                Dial::Via { .. } => return Err(NO_CALL_TUNNEL.to_string()),
            };
            // No ring from here: the ring is the *far* exchange's to send,
            // once ours has carried the request to it. Ringing the target's
            // key at our own exchange would reach nobody, since that is the
            // whole reason this path exists.
            let (client, session, id) =
                engine::establish_cross(endpoint, &signer, &target, wait, &mut bridge).await?;
            engine::call(client, session, id, opts, &mut bridge).await
        }
        .await;

        match &result {
            Ok(_) => tracing::info!("call over"),
            Err(e) => tracing::warn!("call over with trouble: {e}"),
        }
        ending.send_modify(|s| {
            s.phase = Phase::Ended;
            if let Err(e) = &result {
                s.trouble = Some(e.clone());
            }
        });
        (ending_wake)();
        result
    });

    CallHandle {
        stop: stop_tx,
        state: state_rx,
        events: events_rx,
        task,
        ending: hanging_up,
        wake: hanging_up_wake,
    }
}

/// **SIP-39, the other half: answer a call that another exchange carried
/// here.**
///
/// The ring arrived on this identity's event stream as a `CrossCall` naming
/// the caller and the bridge our exchange holds for it. Answering is opening
/// a session back toward the caller *at our own exchange* -- which matches
/// the open to the ringing bridge, so a single rendezvous suffices -- and
/// then the same call as any other. Nothing is dialled: the caller's
/// exchange already did that, which is why the connection this rides is the
/// one this identity already holds.
pub fn spawn_cross_answer(
    dial: impl Into<Dial>,
    signer: SoftwareSigner,
    caller: PubKey,
    wait: u64,
    opts: CallOpts,
    wake: impl Fn() + Send + Sync + 'static,
) -> CallHandle {
    let dial = dial.into();
    let (state_tx, state_rx) = watch::channel(CallState {
        phase: Phase::Connecting,
        peer: Some(caller),
        ..CallState::default()
    });
    let (events_tx, events_rx) = mpsc::unbounded_channel();
    // Asked-to-stop, so hanging up drains and closes rather than dropping
    // the task at its next await; see `CallHandle::stop`.
    let (stop_tx, stop_rx) = watch::channel(false);
    let opts = CallOpts {
        stop: Some(stop_rx),
        ..opts
    };
    let wake = Arc::new(wake);

    let ending = state_tx.clone();
    let ending_wake = wake.clone();
    let hanging_up = state_tx.clone();
    let hanging_up_wake = wake.clone();
    let task = tokio::spawn(async move {
        let mut bridge = Bridge {
            state: state_tx,
            events: events_tx,
            wake,
        };
        let result = async {
            let mut client = match dial {
                Dial::On(held) => {
                    let (client, _) = held.now().ok_or("not connected to the exchange")?;
                    engine::adopt(client, &signer, &mut bridge)?
                }
                Dial::At(e) => engine::connect(e, &signer, &mut bridge).await?,
                Dial::Discover(layers) => {
                    let e = engine::resolve(&layers[..], &mut bridge).await?;
                    engine::connect(e, &signer, &mut bridge).await?
                }
                Dial::Via { .. } => return Err(NO_CALL_TUNNEL.to_string()),
            };
            let (session, id) =
                engine::rendezvous(&mut client, &signer, caller, wait, &mut bridge).await?;
            engine::call(client, session, id, opts, &mut bridge).await
        }
        .await;

        match &result {
            Ok(_) => tracing::info!("call over"),
            Err(e) => tracing::warn!("call over with trouble: {e}"),
        }
        ending.send_modify(|s| {
            s.phase = Phase::Ended;
            if let Err(e) = &result {
                s.trouble = Some(e.clone());
            }
        });
        (ending_wake)();
        result
    });

    CallHandle {
        stop: stop_tx,
        state: state_rx,
        events: events_rx,
        task,
        ending: hanging_up,
        wake: hanging_up_wake,
    }
}

/// **Refuse a call another exchange carried here** (SIP-39). A refusal is a
/// message of its own: the caller is told, rather than left polling until
/// their exchange gives up on ours. On the connection this identity already
/// holds, since that is where the bridge is.
pub async fn decline_cross(held: &Held, bridge: [u8; 16]) -> Result<(), String> {
    let (mut client, _) = held.now().ok_or("not connected to the exchange")?;
    let decline = sqex_proto::session::CallDecline {
        bridge,
        reason: sqex_proto::relay::REASON_DECLINED,
    };
    let (code, body) = client
        .post("/session/decline", decline.encode())
        .await
        .map_err(|e| e.to_string())?;
    if code != 200 {
        return Err(format!(
            "decline failed ({code}): {}",
            String::from_utf8_lossy(&body)
        ));
    }
    Ok(())
}

/// Join a room, on a task of its own.
///
/// A room is not a call with more people in it. Nobody is dialled: holding the
/// secret *is* being a member, so this connects and then keeps a session to
/// each person who turns up. There is no owner, and nobody to answer.
///
/// That difference is worth keeping visible in the interface as well as here.
/// A room's membership cannot be revoked — anyone holding the secret is in, and
/// can pass it on — so leaving somebody out means minting a new room. It must
/// not be made to look like a channel where somebody can be removed.
pub fn spawn_room(
    dial: impl Into<Dial>,
    signer: SoftwareSigner,
    room: RoomId,
    opts: CallOpts,
    wake: impl Fn() + Send + Sync + 'static,
) -> CallHandle {
    let (state_tx, state_rx) = watch::channel(CallState {
        phase: Phase::Connecting,
        room: Some(room),
        ..CallState::default()
    });
    let (events_tx, events_rx) = mpsc::unbounded_channel();
    // Asked-to-stop, so hanging up drains and closes rather than dropping
    // the task at its next await; see `CallHandle::stop`.
    let (stop_tx, stop_rx) = watch::channel(false);
    let opts = CallOpts {
        stop: Some(stop_rx),
        ..opts
    };
    let wake = Arc::new(wake);
    let dial = dial.into();

    let ending = state_tx.clone();
    let ending_wake = wake.clone();
    let hanging_up = state_tx.clone();
    let hanging_up_wake = wake.clone();
    let task = tokio::spawn(async move {
        let mut bridge = Bridge {
            state: state_tx,
            events: events_tx,
            wake,
        };
        let result = async {
            let client = match dial {
                Dial::On(held) => {
                    let (client, _) = held.now().ok_or("not connected to the exchange")?;
                    engine::adopt(client, &signer, &mut bridge)?
                }
                Dial::At(e) => engine::connect(e, &signer, &mut bridge).await?,
                Dial::Discover(layers) => {
                    let e = engine::resolve(&layers[..], &mut bridge).await?;
                    engine::connect(e, &signer, &mut bridge).await?
                }
                Dial::Via { .. } => return Err(NO_CALL_TUNNEL.to_string()),
            };
            engine::room_call(client, &signer, room, opts, &mut bridge).await
        }
        .await;

        ending.send_modify(|s| {
            s.phase = Phase::Ended;
            // Everyone is unreachable once the room is left; leaving them on
            // screen would show a roster of people who cannot hear you.
            s.present.clear();
            s.connecting = 0;
            if let Err(e) = &result {
                s.trouble = Some(e.clone());
            }
        });
        (ending_wake)();
        result
    });

    CallHandle {
        stop: stop_tx,
        state: state_rx,
        events: events_rx,
        task,
        ending: hanging_up,
        wake: hanging_up_wake,
    }
}

/// A direct-message call: straight to the peer if both sides are
/// introduced, and through the exchange's room otherwise.
///
/// The room is what SIP-36 hands both sides, and it is where they meet when
/// the introduction fails on either side -- a room tolerates the two
/// arriving at different moments, which they will when one's dial timed out
/// and the other's accept did. `direct` is the caller's `MEDIA_DIRECT` bit
/// and this side's own setting, both: the bit says the other side will ask,
/// and asking without them costs the wait for nothing.
///
/// The introduction needs the exchange's address, so a borrowed connection
/// lends its endpoint as well; a dialled one resolves it first, as a room
/// does.
pub fn spawn_dm_call(
    dial: impl Into<Dial>,
    signer: SoftwareSigner,
    peer: PubKey,
    room: RoomId,
    direct: bool,
    opts: CallOpts,
    wake: impl Fn() + Send + Sync + 'static,
) -> CallHandle {
    let (state_tx, state_rx) = watch::channel(CallState {
        phase: Phase::Connecting,
        peer: Some(peer),
        room: Some(room),
        ..CallState::default()
    });
    let (events_tx, events_rx) = mpsc::unbounded_channel();
    // Asked-to-stop, so hanging up drains and closes rather than dropping
    // the task at its next await; see `CallHandle::stop`.
    let (stop_tx, stop_rx) = watch::channel(false);
    let opts = CallOpts {
        stop: Some(stop_rx),
        ..opts
    };
    let wake = Arc::new(wake);
    let dial = dial.into();

    let ending = state_tx.clone();
    let ending_wake = wake.clone();
    let hanging_up = state_tx.clone();
    let hanging_up_wake = wake.clone();
    let task = tokio::spawn(async move {
        let mut bridge = Bridge {
            state: state_tx,
            events: events_tx,
            wake,
        };
        let result = async {
            let (client, endpoint) = match dial {
                Dial::On(held) => {
                    let (client, endpoint) = held.now().ok_or("not connected to the exchange")?;
                    (engine::adopt(client, &signer, &mut bridge)?, endpoint)
                }
                Dial::At(e) => (engine::connect(e, &signer, &mut bridge).await?, e),
                Dial::Discover(layers) => {
                    let e = engine::resolve(&layers[..], &mut bridge).await?;
                    (engine::connect(e, &signer, &mut bridge).await?, e)
                }
                Dial::Via { .. } => return Err(NO_CALL_TUNNEL.to_string()),
            };
            if direct {
                match sqex_voice::direct::connect(
                    endpoint,
                    &signer.seed(),
                    peer,
                    sqex_voice::direct::Budget::default(),
                    &mut bridge,
                )
                .await
                {
                    Ok(Some((conn, session, id))) => {
                        return engine::call(conn, session, id, opts, &mut bridge).await;
                    }
                    Ok(None) => bridge.event(Event::Relayed {
                        why: "the other side did not ask to be introduced".into(),
                    }),
                    Err(why) => bridge.event(Event::Relayed { why }),
                }
            } else {
                bridge.event(Event::Relayed {
                    why: "no introduction was asked for".into(),
                });
            }
            engine::room_call(client, &signer, room, opts, &mut bridge).await
        }
        .await;

        match &result {
            Ok(_) => tracing::info!("call over"),
            Err(e) => tracing::warn!("call over with trouble: {e}"),
        }
        ending.send_modify(|s| {
            s.phase = Phase::Ended;
            s.present.clear();
            s.connecting = 0;
            if let Err(e) = &result {
                s.trouble = Some(e.clone());
            }
        });
        (ending_wake)();
        result
    });

    CallHandle {
        stop: stop_tx,
        state: state_rx,
        events: events_rx,
        task,
        ending: hanging_up,
        wake: hanging_up_wake,
    }
}

impl CallHandle {
    /// A handle whose call is whatever `state` says, for drawing one
    /// without placing one. The task behind it does nothing and ends when
    /// hung up.
    pub fn for_test(state: CallState) -> CallHandle {
        let (state_tx, state_rx) = watch::channel(state);
        let (_events_tx, events_rx) = mpsc::unbounded_channel();
        let task = tokio::spawn(std::future::pending());
        // Nothing is listening; the handle only has to be whole.
        let (stop_tx, _stop_rx) = watch::channel(false);
        CallHandle {
            stop: stop_tx,
            state: state_rx,
            events: events_rx,
            task,
            ending: state_tx,
            wake: Arc::new(|| {}),
        }
    }
}

#[cfg(test)]
mod reach_tests {
    use super::*;

    fn configured() -> Vec<sqex_discovery::Layer> {
        vec![sqex_discovery::Layer {
            server: Some("squic.org".into()),
            ..Default::default()
        }]
    }

    // The case that needs a real connection — a slot with one in it — is in
    // `tests/one_identity_one_connection.rs`, where there is an exchange to
    // connect to. A `Held` cannot be filled without one, and a fake that could
    // be would be testing the fake.

    /// An offered slot that has not filled yet is not a connection.
    ///
    /// A chat session offers its slot the moment it starts and fills it a
    /// handshake later. A call placed in that window has to dial: waiting is
    /// the console's trade, not a caller's.
    #[test]
    fn an_empty_slot_falls_back_to_dialling() {
        assert!(matches!(
            Dial::borrowed_or(Some(Held::empty()), configured()),
            Some(Dial::Discover(_))
        ));
    }

    /// Nothing to borrow and nowhere to dial is not a call.
    #[test]
    fn nothing_at_all_is_nothing_to_do() {
        assert!(Dial::borrowed_or(None, Vec::new()).is_none());
        assert!(Dial::borrowed_or(Some(Held::empty()), Vec::new()).is_none());
        assert!(
            Dial::borrowed_or(None, vec![sqex_discovery::Layer::default()]).is_none(),
            "a layer that names no exchange is not an exchange"
        );
    }

    /// With nothing lent, a configured exchange is dialled.
    #[test]
    fn no_connection_and_a_configured_exchange_dials() {
        assert!(matches!(
            Dial::borrowed_or(None, configured()),
            Some(Dial::Discover(_))
        ));
    }
}
