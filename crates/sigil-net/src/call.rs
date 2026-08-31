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

use sqex_voice::engine::{self, CallOpts, Endpoint, Event, Report};
use sqnr_core::{PubKey, SoftwareSigner};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

/// Where a call has got to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    /// Not started.
    #[default]
    Idle,
    /// Dialling the exchange.
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

/// Everything the interface needs to draw a call, and nothing it does not.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CallState {
    pub phase: Phase,
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
            // The rest are narrative: they say what happened, not what is true.
            Event::Pinned { .. }
            | Event::StillWaiting { .. }
            | Event::Roster(_)
            | Event::Draining
            | Event::BadFrame { .. }
            | Event::Reflected(_)
            | Event::CallerGone { .. }
            | Event::Device(_) => {}
        });
        // A closed receiver means the interface has gone; the call carries on
        // regardless, because hanging up is a decision and not a side effect of
        // nobody watching.
        let _ = self.events.send(event);
        (self.wake)();
    }
}

/// A running call.
///
/// Dropping this does **not** end the call — the task owns it. Use
/// [`hang_up`](CallHandle::hang_up), so that ending a call is always something
/// somebody decided.
pub struct CallHandle {
    state: watch::Receiver<CallState>,
    events: mpsc::UnboundedReceiver<Event>,
    task: JoinHandle<Result<(), String>>,
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
        self.task.abort();
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

/// Place a call, on a task of its own.
///
/// `wake` is called whenever anything changes — pass `egui`'s
/// `Context::request_repaint`. Everything here is event-driven precisely so
/// that a silent call costs nothing to display: an interface that redrew at
/// sixty frames a second through an hour of quiet would be the largest single
/// consumer of power in the application.
pub fn spawn_call(
    endpoint: Endpoint,
    signer: SoftwareSigner,
    peer: PubKey,
    wait: u64,
    opts: CallOpts,
    wake: impl Fn() + Send + Sync + 'static,
) -> CallHandle {
    let (state_tx, state_rx) = watch::channel(CallState {
        phase: Phase::Connecting,
        peer: Some(peer),
        ..CallState::default()
    });
    let (events_tx, events_rx) = mpsc::unbounded_channel();
    let wake = Arc::new(wake);

    let ending = state_tx.clone();
    let ending_wake = wake.clone();
    let task = tokio::spawn(async move {
        let mut bridge = Bridge {
            state: state_tx,
            events: events_tx,
            wake,
        };
        let result = async {
            let (client, session, id) =
                engine::establish(endpoint, &signer, peer, wait, &mut bridge).await?;
            engine::call(client, session, id, opts, &mut bridge).await
        }
        .await;

        // Whatever happened, the interface must be told the call is over --
        // otherwise a failed call sits on screen looking like a connecting one
        // forever.
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
        state: state_rx,
        events: events_rx,
        task,
    }
}
