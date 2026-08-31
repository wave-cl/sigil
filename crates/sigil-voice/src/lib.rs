//! Calls and rooms, as a sigil app.

use sigil::app::{App, AppContext, AppResponse};
use sigil::{ColorTheme, tokens};

/// Where you are inside the voice app. Pushed onto the shell's history as an
/// opaque token and handed back to `render_nav`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    Roster,
}

#[derive(Default)]
pub struct VoiceApp {}

impl VoiceApp {
    pub fn new() -> Self {
        Self::default()
    }
}

impl App for VoiceApp {
    fn render(&mut self, _ctx: &mut AppContext<'_>, ui: &mut egui::Ui) -> AppResponse {
        let theme = ColorTheme::current(ui.ctx());
        ui.vertical(|ui| {
            ui.add_space(tokens::SPACING_LG);
            ui.heading("Calls");
            ui.add_space(tokens::SPACING_SM);
            ui.colored_label(theme.text_secondary, "No call in progress.");
        });
        AppResponse::default()
    }

    fn title(&self) -> &str {
        "Calls"
    }
}
