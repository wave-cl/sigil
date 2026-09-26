//! A call, on a screen of its own.
//!
//! Takes plain data rather than anything from the protocol, so this crate stays
//! a set of widgets rather than a second place a call's state machine is
//! understood. The caller maps its own types onto [`Call`] and acts on the
//! [`CallPress`] that comes back.

use sigil::{ColorTheme, Icon, tokens};

/// How wide a control's disc is, and so how far apart the controls sit.
///
/// Larger than [`tokens::BUTTON_LG`], which is a thumb's minimum: these are
/// pressed against an ear, without looking, and one of them ends the call.
const CONTROL: f32 = 56.0;

/// Everything a call card draws.
pub struct Call<'a> {
    /// Their key, or the room's, in full. The mark is drawn from it, and on a
    /// two-party call it is drawn under the name: a name is an assertion and a
    /// key is not (SIP-21).
    pub key: &'a str,
    /// What to call them. Never the only thing on screen.
    pub named: &'a str,
    /// Their picture, when there is one. `None` draws the identicon.
    pub picture: Option<&'a egui::TextureHandle>,
    /// Whose call it is, when that is not the identity being looked at. A
    /// hang-up that ends somebody else's call has to say whose.
    pub whose: Option<&'a str>,
    /// The session is up.
    pub up: bool,
    /// Up, and nothing has arrived from the other side.
    pub deaf: bool,
    pub seconds: u64,
    /// How the sound travels, and why — the word, then the reason.
    pub travel: Option<(&'a str, &'a str)>,
    pub muted: bool,
    /// Where the sound comes out: `Some(true)` the loudspeaker, `Some(false)`
    /// the earpiece. `None` where this device has no such choice, and then no
    /// control is drawn — a disabled one says the app could do something it
    /// cannot.
    pub speaker: Option<bool>,
    /// The microphones that could be chosen, and which is live. Empty draws
    /// no chooser at all: a device list with one entry is not a choice, and a
    /// machine with no microphone has nothing to offer.
    pub microphones: &'a [Mic<'a>],
    /// The engine's own line, when the numbers are open.
    pub stats: Option<&'a str>,
    /// Whether the numbers are open. The clock is the toggle.
    pub detail: bool,
    /// A room's roster. Empty for a two-party call.
    pub present: &'a [crate::roster::Row],
    pub connecting: usize,
    /// Two-party, so the key under the name is a person's. A room's key is a
    /// channel and names nobody, and its members carry their own in the roster.
    pub two_party: bool,
}

/// One microphone a call could capture from.
pub struct Mic<'a> {
    /// The name, spelled as the engine matches it.
    pub name: &'a str,
    /// The one this call is capturing from now.
    pub live: bool,
    /// The one a call that chose nothing would open. Marked, because the
    /// system default and the device a call really uses are often different
    /// -- a connected headset is stepped around deliberately.
    pub fallback: bool,
}

/// What was pressed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CallPress {
    Mute,
    Unmute,
    /// Move the sound to the loudspeaker.
    Speaker,
    /// Move it back to the earpiece.
    Earpiece,
    HangUp,
    /// Open or close the numbers.
    Detail,
    /// Capture from this microphone instead. `None` is whichever one a call
    /// that chose nothing would open.
    Microphone(Option<String>),
}

/// A call's own control: a round target with the mark in it, the word under it,
/// and filled when it is on.
///
/// **Both the mark and the word, which is stricter than the rule.**
/// `sigil::icon::named_control_as` would give a plain text button on a phone,
/// and that is right for a row in a list. These are pressed with the phone
/// against an ear and one of them ends the call, so they keep a finger-sized
/// disc *and* gain the word.
///
/// `said` is the sentence — what pressing does — and reaches the accessibility
/// tree and the tooltip. `drawn` is the one word there is room for under the
/// disc on a screen 360 points wide.
pub fn call_control(
    ui: &mut egui::Ui,
    icon: Icon,
    drawn: &str,
    said: &str,
    tint: Option<egui::Color32>,
    on: bool,
) -> egui::Response {
    let theme = ColorTheme::current(ui.ctx());
    let phone = sigil::Form::of(ui.ctx()).is_phone();
    let colour = tint.unwrap_or(theme.text_primary);

    // Room for the disc and the word under it, everywhere: the word is drawn
    // on every platform, and a rect that did not allow for it would let the
    // next thing below overlap it.
    let width = CONTROL.max(64.0);
    let height = CONTROL + 20.0;
    let (rect, mut response) =
        ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());

    // Said before the visibility guard, as every control in this app does, so
    // a screen reader finds it whether or not it is on screen. `selected` and
    // not `labeled`: the state is drawn, so it is said too, and a test can ask
    // whether this is the muted one without looking at pixels.
    response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, said));

    if ui.is_rect_visible(rect) {
        let disc = egui::Rect::from_center_size(
            egui::pos2(rect.center().x, rect.top() + CONTROL / 2.0),
            egui::vec2(CONTROL, CONTROL),
        );
        let hovered = response.hovered();
        // Filled when it is on, so "muted" is a state you can see across a
        // room and not a tint you have to remember the meaning of.
        let fill = if on {
            colour.linear_multiply(0.30)
        } else if hovered {
            theme.surface_elevated
        } else {
            theme.surface_primary
        };
        ui.painter()
            .circle_filled(disc.center(), CONTROL / 2.0, fill);
        ui.painter().circle_stroke(
            disc.center(),
            CONTROL / 2.0,
            egui::Stroke::new(1.0, if on { colour } else { theme.border_strong }),
        );
        sigil::icon::draw(
            ui.painter(),
            egui::Rect::from_center_size(disc.center(), egui::vec2(CONTROL * 0.45, CONTROL * 0.45)),
            icon,
            colour,
        );
        // **The word under the disc on every platform.** It was a phone-only
        // affordance with a hover in its place on a desktop, and a hover is
        // no use for the control somebody reaches for in a hurry: you have to
        // already know which disc to point at to be told what it is.
        ui.painter().text(
            egui::pos2(rect.center().x, disc.bottom() + 4.0),
            egui::Align2::CENTER_TOP,
            drawn,
            egui::TextStyle::Small.resolve(ui.style()),
            theme.text_muted,
        );
    }
    if !phone {
        response = response.on_hover_text(said);
    }
    response
}

/// Draw the card. Returns what was pressed, if anything.
pub fn call_card(ui: &mut egui::Ui, call: &Call<'_>) -> Option<CallPress> {
    let theme = ColorTheme::current(ui.ctx());
    let mut pressed = None;

    // **The controls first, pinned to the foot.** Drawn in the flow they end up
    // below the roster, and a room of twelve puts Hang up off the bottom of a
    // phone — which sigil-voice already paid for once. A call somebody cannot
    // end is a microphone that stays open.
    egui::Panel::bottom("call_controls")
        .frame(egui::Frame::NONE.inner_margin(egui::Margin::same(tokens::SPACING_MD as i8)))
        .show(ui, |ui| {
            // **Above the discs, and only where there is a choice.** One
            // microphone is not a choice and a machine with none has nothing
            // to offer, so the row is absent rather than empty -- the same
            // rule the routing control follows. Here rather than in the body
            // because a room's roster would scroll it off, and this is a
            // control, not a thing to read.
            if call.microphones.len() > 1 {
                let live = call.microphones.iter().find(|m| m.live);
                let shown = live
                    .map(|m| m.name)
                    .or_else(|| call.microphones.iter().find(|m| m.fallback).map(|m| m.name))
                    .unwrap_or("Default microphone");
                ui.horizontal(|ui| {
                    sigil::icon::draw(
                        ui.painter(),
                        egui::Rect::from_min_size(
                            ui.cursor().min + egui::vec2(0.0, 2.0),
                            egui::vec2(14.0, 14.0),
                        ),
                        Icon::Mic,
                        theme.text_muted,
                    );
                    ui.add_space(18.0);
                    egui::ComboBox::from_id_salt("call_microphone")
                        .width((ui.available_width() - tokens::SPACING_SM).max(0.0))
                        .height(240.0f32.min(ui.available_height().max(120.0) * 0.6))
                        .selected_text(egui::RichText::new(shown).small())
                        .show_ui(ui, |ui| {
                            for mic in call.microphones {
                                // The one a call would open if nobody chose is
                                // marked, because it is often *not* the system
                                // default -- a connected headset is stepped
                                // around on purpose.
                                let label = if mic.fallback {
                                    format!("{} — used by default", mic.name)
                                } else {
                                    mic.name.to_string()
                                };
                                if ui.selectable_label(mic.live, label).clicked() && !mic.live {
                                    pressed =
                                        Some(CallPress::Microphone(Some(mic.name.to_string())));
                                }
                            }
                        });
                });
                ui.add_space(tokens::SPACING_SM);
            }
            ui.vertical_centered(|ui| {
                ui.horizontal(|ui| {
                    let controls = 1 + usize::from(call.speaker.is_some()) + 1;
                    let each = CONTROL.max(64.0) + ui.spacing().item_spacing.x;
                    let room = ui.available_width();
                    let pad = (room - each * controls as f32).max(0.0) / 2.0;
                    ui.add_space(pad);

                    let (icon, drawn, said) = if call.muted {
                        (Icon::MicOff, "Unmute", "Unmute your microphone")
                    } else {
                        (Icon::Mic, "Mute", "Mute your microphone")
                    };
                    if call_control(ui, icon, drawn, said, None, call.muted).clicked() {
                        pressed = Some(if call.muted {
                            CallPress::Unmute
                        } else {
                            CallPress::Mute
                        });
                    }

                    if let Some(on) = call.speaker {
                        let (drawn, said) = if on {
                            ("Earpiece", "Play through the earpiece")
                        } else {
                            ("Speaker", "Play through the loudspeaker")
                        };
                        if call_control(ui, Icon::Sound, drawn, said, None, on).clicked() {
                            pressed = Some(if on {
                                CallPress::Earpiece
                            } else {
                                CallPress::Speaker
                            });
                        }
                    }

                    if call_control(
                        ui,
                        Icon::HangUp,
                        "Hang up",
                        "Hang up",
                        Some(theme.destructive),
                        false,
                    )
                    .clicked()
                    {
                        pressed = Some(CallPress::HangUp);
                    }
                });
            });
        });

    // Everything above is read, not tapped — and a phone raises every row to a
    // thumb's height, which would space these lines out like a menu. Zeroed on
    // the ui they are built from, as `message::centred` does it.
    // **A two-party call has nothing to fill the middle with**, and drawn from
    // the top it left a third of a phone and half a desktop empty below the
    // face. Centred against the height it measured last pass: the first frame
    // draws it high and the second puts it where it belongs, which no repaint
    // ever shows and is why the snapshots run two passes. A room is left at
    // the top, because its roster is what fills the space.
    let plain = call.present.is_empty() && call.connecting == 0;
    let measured = ui.id().with("call_body_height");
    let free = ui.available_height();
    if plain && let Some(was) = ui.data(|d| d.get_temp::<f32>(measured)) {
        ui.add_space(((free - was) / 2.0).max(0.0));
    }
    let body = ui
        .scope(|ui| {
            ui.spacing_mut().interact_size.y = 0.0;
            ui.vertical_centered(|ui| {
                ui.add_space(tokens::SPACING_XL);
                crate::avatar(ui, call.key, call.picture, tokens::AVATAR_XL);
                ui.add_space(tokens::SPACING_MD);
                ui.add(egui::Label::new(egui::RichText::new(call.named).heading()).truncate());

                if call.two_party {
                    ui.add(
                        egui::Label::new(egui::RichText::new(call.key).monospace().small())
                            .wrap()
                            .selectable(true),
                    );
                }
                ui.add_space(tokens::SPACING_SM);

                ui.horizontal(|ui| {
                    let gap = (ui.available_width() - 120.0).max(0.0) / 2.0;
                    ui.add_space(gap);
                    crate::dot(
                        ui,
                        call.up && !call.deaf,
                        theme.success,
                        theme.warning,
                        if call.deaf {
                            "connected, but nothing is arriving"
                        } else if call.up {
                            "connected"
                        } else {
                            "connecting"
                        },
                    );
                    let said = match (call.up, call.whose) {
                        (true, None) => "In a call".to_string(),
                        (true, Some(who)) => format!("In a call as {who}"),
                        (false, None) => "Connecting…".to_string(),
                        (false, Some(who)) => format!("Connecting… as {who}"),
                    };
                    ui.colored_label(
                        if call.up {
                            theme.success
                        } else {
                            theme.warning
                        },
                        egui::RichText::new(said).small(),
                    );
                });

                // Monospace so the digits do not shuffle the row every second.
                let clock = format!("{:02}:{:02}", call.seconds / 60, call.seconds % 60);
                if ui
                    .add(
                        egui::Button::new(egui::RichText::new(clock).monospace().heading().color(
                            if call.detail {
                                theme.accent
                            } else {
                                theme.text_muted
                            },
                        ))
                        .frame(false),
                    )
                    .clicked()
                {
                    pressed = Some(CallPress::Detail);
                }

                if let Some((word, why)) = call.travel {
                    ui.colored_label(theme.text_muted, egui::RichText::new(word).small());
                    // Drawn on a phone, hovered on a desktop: the rule the rest of
                    // this app follows, because a phone cannot hover.
                    if !why.is_empty() {
                        if sigil::Form::of(ui.ctx()).is_phone() {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(why).small().color(theme.text_muted),
                                )
                                .wrap(),
                            );
                        } else {
                            ui.label("").on_hover_text(why);
                        }
                    }
                }

                if call.deaf {
                    ui.add_space(tokens::SPACING_SM);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new("Nothing is coming through from the other side.")
                                .small()
                                .color(theme.warning),
                        )
                        .wrap(),
                    );
                }

                if let Some(stats) = call.stats.filter(|_| call.detail || call.deaf) {
                    ui.add_space(tokens::SPACING_SM);
                    ui.add(
                        egui::Label::new(
                            egui::RichText::new(stats).small().color(theme.text_muted),
                        )
                        .wrap(),
                    );
                }
            });

            if !call.present.is_empty() || call.connecting > 0 {
                ui.add_space(tokens::SPACING_MD);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    // **The same rule as the line above.** `stats` is drawn
                    // only when the numbers are open or the call is deaf, and
                    // a per-person copy of the same numbers under every face
                    // in the room is the same thing said twelve times over.
                    crate::roster(ui, call.present, call.connecting, call.detail || call.deaf);
                });
            }
        })
        .response
        .rect
        .height();
    if plain {
        ui.data_mut(|d| d.insert_temp(measured, body));
    }

    pressed
}
