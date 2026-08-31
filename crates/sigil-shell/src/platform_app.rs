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
use sigil_platform::{Platform, Support};

pub struct PlatformApp {
    platform: Platform,
}

impl PlatformApp {
    pub fn new(platform: Platform) -> PlatformApp {
        PlatformApp { platform }
    }
}

impl App for PlatformApp {
    fn render(&mut self, _ctx: &mut AppContext<'_>, ui: &mut egui::Ui) -> AppResponse {
        let theme = ColorTheme::current(ui.ctx());
        ui.heading("This desktop");
        ui.colored_label(theme.text_secondary, self.platform.session().describe());
        ui.add_space(tokens::SPACING_MD);

        if !self.platform.can_reach_you_when_away() {
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

        for capability in self.platform.capabilities() {
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
        let starts = self.platform.autostart.enabled();
        let can = self.platform.autostart.support().is_yes();
        ui.add_enabled_ui(can, |ui| {
            let mut on = starts;
            if ui.checkbox(&mut on, "Start sigil at login").changed() {
                let _ = self.platform.autostart.set(on);
            }
        });
        if let Support::No(why) = self.platform.autostart.support() {
            ui.colored_label(theme.destructive, why);
        }
        AppResponse::default()
    }

    fn title(&self) -> &str {
        "Desktop"
    }
}
