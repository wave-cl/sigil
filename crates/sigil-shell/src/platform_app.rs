//! What this desktop can do, drawn as a list.
//!
//! An app rather than a corner of settings, because it is the answer to "why
//! did that not happen", and somebody looking for that should be able to find
//! it without knowing which pane it hides in.
//!
//! The rule it exists to serve: nothing is silently inert. Every row says what
//! sigil uses the capability *for*, so an unavailable one tells somebody what
//! they are losing rather than only that something is missing.

use sigil::app::{App, AppContext, AppResponse};
use sigil::{ColorTheme, tokens};
use sigil_platform::{Capability, Platform, Support};

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
}

impl Report {
    /// Read the real desktop.
    pub fn of(platform: &Platform) -> Report {
        Report {
            session: platform.session().describe().to_string(),
            capabilities: platform.capabilities(),
            reachable_when_away: platform.can_reach_you_when_away(),
            autostart: platform.autostart.support().clone(),
            autostart_enabled: platform.autostart.enabled(),
        }
    }
}

pub struct PlatformApp {
    platform: Option<Platform>,
    report: Report,
}

impl PlatformApp {
    pub fn new(platform: Platform) -> PlatformApp {
        let report = Report::of(&platform);
        PlatformApp {
            platform: Some(platform),
            report,
        }
    }

    /// A pane over a fixed report, with no desktop behind it.
    ///
    /// For snapshots, which must not depend on the machine that took them. The
    /// autostart checkbox is inert here: there is nothing to enable.
    #[doc(hidden)]
    pub fn from_report(report: Report) -> PlatformApp {
        PlatformApp {
            platform: None,
            report,
        }
    }
}

impl App for PlatformApp {
    fn render(&mut self, _ctx: &mut AppContext<'_>, ui: &mut egui::Ui) -> AppResponse {
        let theme = ColorTheme::current(ui.ctx());
        ui.heading("This desktop");
        ui.colored_label(theme.text_secondary, &self.report.session);
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

    fn title(&self) -> &str {
        "Desktop"
    }
}
