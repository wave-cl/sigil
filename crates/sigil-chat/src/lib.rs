//! Messaging, as a sigil app.

use sigil::app::{App, AppContext, AppResponse};
use sigil::{ColorTheme, tokens};

/// Where you are inside the chat app.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Route {
    Conversations,
}

#[derive(Default)]
pub struct ChatApp {}

impl ChatApp {
    pub fn new() -> Self {
        Self::default()
    }
}

impl App for ChatApp {
    fn render(&mut self, _ctx: &mut AppContext<'_>, ui: &mut egui::Ui) -> AppResponse {
        let theme = ColorTheme::current(ui.ctx());
        ui.vertical(|ui| {
            ui.add_space(tokens::SPACING_LG);
            ui.heading("Chat");
            ui.add_space(tokens::SPACING_SM);
            ui.colored_label(theme.text_secondary, "No conversations yet.");
        });
        AppResponse::default()
    }

    fn title(&self) -> &str {
        "Chat"
    }
}
