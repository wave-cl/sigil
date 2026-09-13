//! What this desktop can do, drawn as a list -- and what version of sigil
//! this is, with the newer one when there is one.
//!
//! An app rather than a corner of settings, because it is the answer to "why
//! did that not happen", and somebody looking for that should be able to find
//! it without knowing which pane it hides in.
//!
//! The rule it exists to serve: nothing is silently inert. Every row says what
//! sigil uses the capability *for*, so an unavailable one tells somebody what
//! they are losing rather than only that something is missing. The update
//! block follows the same rule: a copy that cannot update itself says why,
//! in the place the button would have been.

use sigil::app::{App, AppContext, AppResponse, TabNotifications};
use sigil::{ColorTheme, tokens};
use sigil_platform::{Capability, Platform, Support};
use sigil_update::{Install, UpdateState, Updater, Version};

/// What this pane draws, separated from where it came from.
///
/// The pane reports *what this machine can do*, which is by construction
/// different on every machine — so a snapshot of it taken on one platform can
/// never match another. Mine passed on macOS and failed in CI with eleven
/// thousand differing pixels, which was not a rendering difference at all: the
/// two machines genuinely have different capabilities and the pane was
/// correctly saying so.
///
/// Splitting the data out makes the pane a pure renderer, so a snapshot can be
/// taken of a fixed report and test the *layout* rather than the machine. It is
/// the same split as the roster widget, for the same reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub session: String,
    pub capabilities: Vec<Capability>,
    pub reachable_when_away: bool,
    pub autostart: Support,
    pub autostart_enabled: bool,
    /// `0.1.5`: what this build is.
    pub version: String,
    /// How this copy was installed, which is how a newer one takes its
    /// place -- or why it cannot.
    pub install: Install,
    /// Where the update stands. Copied from the updater every pass.
    pub update: UpdateState,
}

impl Report {
    /// Read the real desktop.
    pub fn of(platform: &Platform, install: Install) -> Report {
        Report {
            session: platform.session().describe().to_string(),
            capabilities: platform.capabilities(),
            reachable_when_away: platform.can_reach_you_when_away(),
            autostart: platform.autostart.support().clone(),
            autostart_enabled: platform.autostart.enabled(),
            version: Version::current().to_string(),
            install,
            update: UpdateState::Unknown,
        }
    }
}

/// A check five seconds after launch -- after the window is up and the
/// accounts are open, so a slow answer never delays either -- and once a
/// day after that.
const FIRST_CHECK: std::time::Duration = std::time::Duration::from_secs(5);
const EVERY: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

pub struct PlatformApp {
    platform: Option<Platform>,
    report: Report,
    updater: Option<Updater>,
    /// The version a notification has been posted about, so a newer
    /// release is announced once and not on every pass.
    announced: Option<Version>,
    /// Why Restart did not work, when it did not.
    restart_trouble: Option<String>,
}

impl PlatformApp {
    /// `api_base` is where releases are asked about --
    /// [`sigil_update::release::GITHUB_API`] unless somebody is testing the
    /// path against a server of their own. `wake` is called from the
    /// update thread whenever there is something new to draw.
    pub fn new(
        platform: Platform,
        api_base: &str,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> PlatformApp {
        let install = sigil_update::install::detect();
        let report = Report::of(&platform, install.clone());
        let updater = install.is_supported().then(|| {
            Updater::start(
                sigil_update::release::Client::new(api_base),
                sigil_update::manifest::verifying_key(&sigil_update::PUBLIC_KEY),
                install,
                FIRST_CHECK,
                EVERY,
                wake,
            )
        });
        PlatformApp {
            platform: Some(platform),
            report,
            updater,
            announced: None,
            restart_trouble: None,
        }
    }

    /// A pane over a fixed report, with no desktop behind it.
    ///
    /// For snapshots and the pane's own tests, which must not depend on the
    /// machine that took them. The autostart checkbox and the update
    /// buttons are inert here: there is nothing to enable and nothing
    /// checking.
    #[doc(hidden)]
    pub fn from_report(report: Report) -> PlatformApp {
        PlatformApp {
            platform: None,
            report,
            updater: None,
            announced: None,
            restart_trouble: None,
        }
    }

    fn update_ui(&mut self, ui: &mut egui::Ui, theme: &ColorTheme) {
        ui.horizontal(|ui| {
            ui.strong(format!("sigil {}", self.report.version));
            ui.colored_label(theme.text_muted, self.report.install.describe());
        });
        let state = self.report.update.clone();
        match &state {
            UpdateState::Unknown => {
                if self.report.install.is_supported() {
                    ui.colored_label(theme.text_secondary, "Not checked for a newer version yet.");
                }
            }
            UpdateState::Checking => {
                ui.colored_label(theme.text_secondary, "Checking for a newer version…");
            }
            UpdateState::UpToDate { .. } => {
                ui.colored_label(theme.text_secondary, "Up to date. sigil checks once a day.");
            }
            UpdateState::Unsigned { version, notes_url } => {
                ui.colored_label(
                    theme.warning,
                    format!(
                        "sigil {version} is on GitHub, but that release is not signed, \
                         so sigil will not install it itself."
                    ),
                );
                ui.hyperlink_to("See the release", notes_url);
            }
            UpdateState::Available {
                version, notes_url, ..
            } => {
                ui.colored_label(theme.text_primary, format!("sigil {version} is available."));
                ui.hyperlink_to("What changed", notes_url);
            }
            UpdateState::Downloading {
                version,
                done,
                total,
            } => {
                let frac = if *total > 0 {
                    *done as f32 / *total as f32
                } else {
                    0.0
                };
                ui.add(
                    egui::ProgressBar::new(frac)
                        .desired_width(320.0)
                        .text(format!(
                            "Fetching sigil {version}… {} of {}",
                            mib(*done),
                            mib(*total)
                        )),
                );
            }
            UpdateState::Installing { version } => {
                ui.colored_label(theme.text_secondary, format!("Installing sigil {version}…"));
            }
            UpdateState::Ready { version } => {
                ui.colored_label(
                    theme.text_primary,
                    format!("sigil {version} is installed. Restart to use it."),
                );
            }
            UpdateState::Failed { why } => {
                ui.colored_label(theme.destructive, format!("The update failed: {why}"));
            }
            UpdateState::Unreachable { why } => {
                ui.colored_label(
                    theme.warning,
                    format!("sigil could not reach GitHub to check for a newer version: {why}"),
                );
            }
        }
        if let Some(why) = &self.restart_trouble {
            ui.colored_label(theme.destructive, format!("Could not restart: {why}"));
        }
        if !self.report.install.is_supported() {
            return;
        }
        ui.horizontal(|ui| self.update_buttons(ui, &state, true));
    }

    /// Start the new copy once this one is gone, then go.
    fn restart(&mut self, ctx: &egui::Context) {
        let Some(target) = sigil_update::relaunch::target(&self.report.install) else {
            self.restart_trouble = Some("nothing to start".into());
            return;
        };
        match sigil_update::relaunch::spawn_relaunch(std::process::id(), &target) {
            Ok(()) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Err(e) => self.restart_trouble = Some(e.to_string()),
        }
    }

    /// The buttons for the update's state, drawn wherever the state is:
    /// on the Desktop pane and in the band across the window.
    /// `with_check` adds Check now, which belongs on the pane and not in
    /// the band: the band exists for the one thing to press.
    fn update_buttons(&mut self, ui: &mut egui::Ui, state: &UpdateState, with_check: bool) {
        let live = self.updater.is_some();
        if matches!(state, UpdateState::Available { .. }) {
            // The version is in the sentence beside it; the button is the verb.
            if ui.add_enabled(live, egui::Button::new("Update")).clicked()
                && let Some(updater) = &self.updater
            {
                updater.update();
            }
        } else if matches!(state, UpdateState::Ready { .. })
            && ui.add_enabled(live, egui::Button::new("Restart")).clicked()
        {
            let ctx = ui.ctx().clone();
            self.restart(&ctx);
        }
        if with_check
            && state.can_check()
            && ui
                .add_enabled(live, egui::Button::new("Check now"))
                .clicked()
            && let Some(updater) = &self.updater
        {
            updater.check_now();
        }
    }
}

/// `12.3 MiB`.
fn mib(bytes: u64) -> String {
    format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
}

impl App for PlatformApp {
    fn runs_unopened(&self) -> bool {
        // The check runs whether or not anybody has looked here; the badge
        // on the tab is how they learn to.
        true
    }

    fn update(&mut self, ctx: &mut AppContext<'_>, _egui_ctx: &egui::Context) {
        let Some(updater) = &self.updater else {
            return;
        };
        self.report.update = updater.state();
        if let UpdateState::Available { version, .. } = &self.report.update
            && self.announced != Some(*version)
        {
            self.announced = Some(*version);
            ctx.notify.post(
                &format!("sigil {version} is available"),
                "Open the Desktop tab and press Update.",
            );
        }
    }

    fn render(&mut self, _ctx: &mut AppContext<'_>, ui: &mut egui::Ui) -> AppResponse {
        let theme = ColorTheme::current(ui.ctx());
        ui.heading("This desktop");
        ui.colored_label(theme.text_secondary, &self.report.session);
        ui.add_space(tokens::SPACING_MD);

        self.update_ui(ui, &theme);
        ui.add_space(tokens::SPACING_MD);
        ui.separator();
        ui.add_space(tokens::SPACING_MD);

        if !self.report.reachable_when_away {
            // The one combination worth shouting about: with neither
            // notifications nor a tray, sigil is a telephone only while it is
            // on screen, and somebody should learn that here rather than by
            // missing a call.
            ui.colored_label(
                theme.warning,
                "Calls can only reach you while this window is open on this desktop.",
            );
            ui.add_space(tokens::SPACING_MD);
        }

        for capability in &self.report.capabilities {
            ui.horizontal(|ui| {
                let available = capability.support.is_yes();
                sigil_ui::dot(
                    ui,
                    available,
                    theme.success,
                    theme.text_muted,
                    if available {
                        "available"
                    } else {
                        "unavailable"
                    },
                );
                ui.strong(capability.name);
            });
            ui.colored_label(theme.text_secondary, capability.what);
            if let Some(why) = capability.support.reason() {
                // The reason, not merely the fact. A row that said only
                // "unavailable" would send somebody looking for a cause that is
                // written down right here.
                ui.colored_label(theme.destructive, why);
            }
            ui.add_space(tokens::SPACING_SM);
        }

        ui.add_space(tokens::SPACING_MD);
        ui.separator();
        let starts = self.report.autostart_enabled;
        let can = self.report.autostart.is_yes() && self.platform.is_some();
        ui.add_enabled_ui(can, |ui| {
            let mut on = starts;
            if ui.checkbox(&mut on, "Start sigil at login").changed()
                && let Some(platform) = &self.platform
            {
                // Best effort, and the checkbox reflects what the desktop
                // actually holds on the next pass rather than what was asked.
                if platform.autostart.set(on).is_ok() {
                    self.report.autostart_enabled = on;
                }
            }
        });
        if let Support::No(why) = &self.report.autostart {
            ui.colored_label(theme.destructive, why);
        }
        AppResponse::default()
    }

    fn has_notice(&self) -> bool {
        // From the moment there is something to press until it has been:
        // Available, the fetch and the install, Ready -- and a failure of
        // any of those, which would otherwise vanish from under the person
        // who pressed the button.
        matches!(
            self.report.update,
            UpdateState::Available { .. }
                | UpdateState::Downloading { .. }
                | UpdateState::Installing { .. }
                | UpdateState::Ready { .. }
                | UpdateState::Failed { .. }
        )
    }

    /// One line across the window: what there is, and the button for it at
    /// the right-hand end.
    fn notice_ui(&mut self, _ctx: &mut AppContext<'_>, ui: &mut egui::Ui) {
        let theme = ColorTheme::current(ui.ctx());
        let state = self.report.update.clone();
        // One rectangle, a button tall, allocated before anything is placed
        // in it: a `horizontal` centres each thing against the height it knew
        // when that thing was placed, so a sentence put down before the
        // button beside it sat a few pixels above the button's middle.
        let row = egui::vec2(ui.available_width(), tokens::BUTTON_MD);
        ui.allocate_ui_with_layout(
            row,
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                match &state {
                    UpdateState::Available { version, .. } => {
                        ui.strong(format!("sigil {version} is available."));
                    }
                    UpdateState::Downloading {
                        version,
                        done,
                        total,
                    } => {
                        let frac = if *total > 0 {
                            *done as f32 / *total as f32
                        } else {
                            0.0
                        };
                        ui.label(format!("Fetching sigil {version}…"));
                        ui.add(egui::ProgressBar::new(frac).desired_width(200.0));
                    }
                    UpdateState::Installing { version } => {
                        ui.label(format!("Installing sigil {version}…"));
                    }
                    UpdateState::Ready { version } => {
                        ui.strong(format!("sigil {version} is installed."));
                        ui.label("Restart to use it.");
                    }
                    UpdateState::Failed { why } => {
                        ui.colored_label(theme.destructive, format!("The update failed: {why}"));
                    }
                    _ => {}
                }
                if let Some(why) = &self.restart_trouble {
                    ui.colored_label(theme.destructive, format!("Could not restart: {why}"));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    self.update_buttons(ui, &state, false);
                });
            },
        );
    }

    fn tab_notifications(&self) -> TabNotifications {
        // One mark: there is something to press here.
        match self.report.update {
            UpdateState::Available { .. } | UpdateState::Ready { .. } => TabNotifications::count(1),
            _ => TabNotifications::default(),
        }
    }

    fn title(&self) -> &str {
        "Desktop"
    }

    fn icon(&self) -> sigil::Icon {
        sigil::Icon::Device
    }
}
