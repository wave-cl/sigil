//! The thread that asks, and what it has to say.
//!
//! One thread, `sigil-update`, sleeping between checks and woken by a
//! command. Every change of state calls `wake`, which is how the window
//! learns to repaint; nothing here touches the interface, and nothing here
//! relaunches -- that is the interface's to do, on its own thread, when
//! somebody presses the button.

use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use ed25519_dalek::VerifyingKey;

use crate::download;
use crate::install::{self, Install};
use crate::manifest::{self, Manifest};
use crate::release::{self, Client, Latest};
use crate::{Error, Version};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateState {
    /// Nothing asked yet.
    Unknown,
    Checking,
    UpToDate {
        checked_at: SystemTime,
    },
    /// A newer release exists but carries no signed manifest, so sigil
    /// will not install it itself. The release before signing began looks
    /// like this from the release after -- it is not a fault.
    Unsigned {
        version: Version,
        notes_url: String,
    },
    Available {
        version: Version,
        notes_url: String,
        asset: String,
    },
    Downloading {
        version: Version,
        done: u64,
        total: u64,
    },
    Installing {
        version: Version,
    },
    /// Installed; the button reads Restart.
    Ready {
        version: Version,
    },
    Failed {
        why: String,
    },
    /// The network, not the release: a timeout, no route, GitHub saying no.
    Unreachable {
        why: String,
    },
}

impl UpdateState {
    /// A check is worth running now: not while one is under way, not while
    /// an install is, and not once one has finished -- a check after that
    /// would say "up to date" about the copy that is about to be replaced.
    pub fn can_check(&self) -> bool {
        !matches!(
            self,
            UpdateState::Checking
                | UpdateState::Downloading { .. }
                | UpdateState::Installing { .. }
                | UpdateState::Ready { .. }
        )
    }
}

pub enum Command {
    CheckNow,
    Update,
}

/// What a check found, kept for the update that may follow it.
#[derive(Clone, Debug)]
pub struct Found {
    pub latest: Latest,
    pub manifest: Manifest,
    pub version: Version,
    pub asset: String,
}

pub struct Updater {
    state: Arc<Mutex<UpdateState>>,
    cmds: mpsc::Sender<Command>,
}

impl Updater {
    /// Start the thread. `first_delay` before the first check, `every`
    /// between checks after that.
    pub fn start(
        client: Client,
        key: VerifyingKey,
        install: Install,
        first_delay: Duration,
        every: Duration,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> Updater {
        let state = Arc::new(Mutex::new(UpdateState::Unknown));
        let (cmds, rx) = mpsc::channel();
        let shared = state.clone();
        let spawned = std::thread::Builder::new()
            .name("sigil-update".into())
            .spawn(move || {
                let mut worker = Worker {
                    client,
                    key,
                    install,
                    state: shared,
                    wake: Box::new(wake),
                    found: None,
                };
                let mut next = Instant::now() + first_delay;
                loop {
                    let wait = next.saturating_duration_since(Instant::now());
                    match rx.recv_timeout(wait) {
                        Ok(Command::CheckNow) => {
                            worker.check();
                            next = Instant::now() + every;
                        }
                        Ok(Command::Update) => worker.update(),
                        Err(RecvTimeoutError::Timeout) => {
                            worker.check();
                            next = Instant::now() + every;
                        }
                        Err(RecvTimeoutError::Disconnected) => return,
                    }
                }
            });
        if let Err(e) = spawned {
            tracing::warn!("could not start the update thread: {e}");
        }
        Updater { state, cmds }
    }

    pub fn state(&self) -> UpdateState {
        self.state.lock().unwrap().clone()
    }

    pub fn check_now(&self) {
        let _ = self.cmds.send(Command::CheckNow);
    }

    pub fn update(&self) {
        let _ = self.cmds.send(Command::Update);
    }
}

struct Worker {
    client: Client,
    key: VerifyingKey,
    install: Install,
    state: Arc<Mutex<UpdateState>>,
    wake: Box<dyn Fn() + Send + Sync>,
    found: Option<Found>,
}

impl Worker {
    fn set(&self, state: UpdateState) {
        *self.state.lock().unwrap() = state;
        (self.wake)();
    }

    fn check(&mut self) {
        if !self.state.lock().unwrap().can_check() {
            return;
        }
        self.set(UpdateState::Checking);
        let (state, found) = check(&self.client, &self.key, &self.install, Version::current());
        self.found = found;
        tracing::info!("update check: {state:?}");
        self.set(state);
    }

    fn update(&mut self) {
        let Some(found) = self.found.clone() else {
            return;
        };
        if !matches!(
            self.state.lock().unwrap().clone(),
            UpdateState::Available { .. }
        ) {
            return;
        }
        let version = found.version;
        let outcome = self.fetch_and_install(&found);
        self.set(match outcome {
            Ok(()) => UpdateState::Ready { version },
            Err(e) => failed(e),
        });
    }

    fn fetch_and_install(&self, found: &Found) -> Result<(), Error> {
        let name = &found.asset;
        let version = found.version;
        let api_asset = found
            .latest
            .asset(name)
            .ok_or_else(|| Error::NoBuild(name.clone()))?;
        let expect = found
            .manifest
            .assets
            .get(name)
            .ok_or_else(|| Error::NoBuild(name.clone()))?;
        let staging = download::staging_dir();
        let _ = std::fs::remove_dir_all(&staging);
        let file = download::download(
            &self.client,
            &api_asset.browser_download_url,
            name,
            expect,
            &staging,
            &|done, total| {
                self.set(UpdateState::Downloading {
                    version,
                    done,
                    total,
                })
            },
        )?;
        // Belt and braces: what the stream hashed is what is on the disc.
        found.manifest.check_file(name, &file)?;
        self.set(UpdateState::Installing { version });
        match &self.install {
            Install::MacBundle { app } => {
                let beside = download::staging_dir_beside(app);
                let _ = std::fs::remove_dir_all(&beside);
                let result = install::unpack(&file, &beside)
                    .and_then(|new_app| install::install_mac_bundle(&new_app, app));
                let _ = std::fs::remove_dir_all(&beside);
                result?;
            }
            Install::LinuxBinary { exe } => {
                let new = install::unpack(&file, &staging.join("unpacked"))?;
                install::install_linux_binary(&new, exe)?;
            }
            Install::LinuxPackage { kind } => install::install_package(*kind, &file)?,
            Install::Unsupported { why } => return Err(Error::Install(why.clone())),
        }
        let _ = std::fs::remove_dir_all(&staging);
        Ok(())
    }
}

/// One check, start to finish: the latest release, whether it is newer,
/// whether it is signed, whether it has a build for this install.
pub fn check(
    client: &Client,
    key: &VerifyingKey,
    install: &Install,
    current: Version,
) -> (UpdateState, Option<Found>) {
    match look(client, key, install, current) {
        Ok((state, found)) => (state, found),
        Err(e) => (failed(e), None),
    }
}

fn look(
    client: &Client,
    key: &VerifyingKey,
    install: &Install,
    current: Version,
) -> Result<(UpdateState, Option<Found>), Error> {
    let latest = client.latest()?;
    let version = Version::parse(&latest.tag_name).ok_or_else(|| {
        Error::BadManifest(format!(
            "the latest release is tagged {:?}",
            latest.tag_name
        ))
    })?;
    if version <= current {
        return Ok((
            UpdateState::UpToDate {
                checked_at: SystemTime::now(),
            },
            None,
        ));
    }
    let (json_name, sig_name) = manifest::file_names(&latest.tag_name);
    let (Some(json), Some(sig)) = (latest.asset(&json_name), latest.asset(&sig_name)) else {
        return Ok((
            UpdateState::Unsigned {
                version,
                notes_url: latest.html_url.clone(),
            },
            None,
        ));
    };
    let bytes = client.fetch_small(&json.browser_download_url)?;
    let sig = client.fetch_small(&sig.browser_download_url)?;
    let sig = String::from_utf8(sig).map_err(|_| Error::BadSignature)?;
    let manifest = manifest::verify(key, &bytes, &sig)?;
    if manifest.tag != latest.tag_name {
        return Err(Error::BadManifest(format!(
            "signed for {}, published as {}",
            manifest.tag, latest.tag_name
        )));
    }
    let asset = release::asset_name(&latest.tag_name, install, std::env::consts::ARCH)
        .ok_or_else(|| Error::Install(install.describe()))?;
    if !manifest.assets.contains_key(&asset) || latest.asset(&asset).is_none() {
        return Err(Error::NoBuild(format!(
            "{} {}",
            std::env::consts::ARCH,
            asset.rsplit('.').next().unwrap_or("")
        )));
    }
    let notes_url = latest.html_url.clone();
    Ok((
        UpdateState::Available {
            version,
            notes_url,
            asset: asset.clone(),
        },
        Some(Found {
            latest,
            manifest,
            version,
            asset,
        }),
    ))
}

/// The network's failures and the release's are shown apart: one says try
/// later, the other says something is wrong.
fn failed(e: Error) -> UpdateState {
    match e {
        Error::Unreachable(why) => UpdateState::Unreachable { why },
        Error::Http(status) => UpdateState::Unreachable {
            why: format!("GitHub answered {status}"),
        },
        other => UpdateState::Failed {
            why: other.to_string(),
        },
    }
}
