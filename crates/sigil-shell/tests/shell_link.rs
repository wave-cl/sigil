//! `sigil://` links, end to end through the real `Shell::ui`.
//!
//! # Why this is its own test binary
//!
//! The queue a link waits in is one per process -- it has to be: whatever
//! hands a link over (Android's activity, the command line) runs on somebody
//! else's thread, long before any window exists. The shell drains it every
//! pass, so **any** `Shell::ui` in the process takes whatever is in it. With
//! this test in `shell_ui.rs` it passed alone and failed in the workspace
//! run, because one of the other thirty-seven shells drained the link first.
//!
//! There is one shell in a real process, so this is a property of the test
//! binary and not of the code. A test binary of its own is the fix that says
//! so; serialising with a lock would not, since the thief holds no lock.

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use sigil::app::{App, AppContext, AppResponse};
use sigil::theme;

/// An app that records the links it is given and draws its name.
struct Stub {
    took: std::rc::Rc<std::cell::RefCell<Vec<sigil::Link>>>,
}

impl App for Stub {
    fn title(&self) -> &str {
        "Calls"
    }
    fn render(&mut self, _ctx: &mut AppContext<'_>, ui: &mut egui::Ui) -> AppResponse {
        ui.heading("Calls");
        AppResponse::default()
    }
    /// Recorded rather than acted on, which is all this needs to see.
    fn follow(&mut self, _ctx: &mut AppContext<'_>, link: &sigil::Link) -> bool {
        self.took.borrow_mut().push(link.clone());
        true
    }
}

/// Everything the interface says, as one string. Both `label` and `value`:
/// accesskit puts an interactive widget's text in one and a plain one's in
/// the other, so reading only labels sees buttons and no prose.
fn said(h: &Harness<'static>) -> String {
    fn walk(node: egui_kittest::Node<'_>, out: &mut Vec<String>) {
        let n = node.accesskit_node();
        if let Some(l) = n.label() {
            out.push(l.to_string());
        }
        if let Some(v) = n.value() {
            out.push(v.to_string());
        }
        for c in node.children() {
            walk(c, out);
        }
    }
    let mut found = Vec::new();
    walk(h.root(), &mut found);
    found.join(" | ")
}

/// **A `sigil://` link is a question, and the answer decides everything.**
///
/// A link is a thing somebody else wrote and put where you would press it,
/// and the most dangerous one is the least dramatic: `sigil://room/<secret>`
/// names a room whose membership *is* holding the secret, so joining one
/// silently cannot be taken back and there is nobody to remove you.
///
/// It was parsed and then dropped, on both the desktop and the phone: the
/// link opened a window that said nothing about it, and the only trace was a
/// line in the log. Now it reaches the interface, which asks, and nothing
/// happens until somebody says yes.
///
/// Three things, because each fails on its own: that the question is asked,
/// that No does nothing, and that Yes reaches the app.
#[test]
fn a_sigil_link_is_asked_about_before_anything_happens() {
    // A real key, from an identity rather than from thirty-two bytes: a
    // link's key has to parse as one, and `parse` is deliberately strict.
    fn key() -> String {
        sigil::Account::unlocked_for_test([7u8; 32])
            .unlocked()
            .expect("unlocked")
            .me()
            .to_string()
    }
    fn shell_with() -> (
        Harness<'static>,
        std::rc::Rc<std::cell::RefCell<Vec<sigil::Link>>>,
    ) {
        let took: std::rc::Rc<std::cell::RefCell<Vec<sigil::Link>>> = Default::default();
        let apps: Vec<Box<dyn App>> = vec![Box::new(Stub { took: took.clone() })];
        let mut shell =
            sigil_shell::Shell::new(apps, None).with_accounts(sigil::accounts::Accounts::of(vec![
                sigil::Account::unlocked_for_test([9u8; 32]),
            ]));
        let h = Harness::builder()
            .with_size(egui::vec2(900.0, 700.0))
            .with_step_dt(0.05)
            .build_ui(move |ui| {
                let ctx = ui.ctx().clone();
                theme::install(&ctx, theme::light(), theme::dark());
                ctx.set_theme(egui::Theme::Dark);
                shell.ui(ui);
            });
        (h, took)
    }

    // Nothing left over from another test in this binary: the queue is one
    // per process.
    let _ = sigil_platform::deeplink::offered();

    // **The question.** A room, because that is the one whose wording
    // matters most.
    sigil_platform::deeplink::offer("sigil://room/TestRoomSecretNotARea1RoomDoNotUseAAAAAAAAAA")
        .expect("a good link");
    let (mut h, took) = shell_with();
    h.run();
    h.run();
    let asked = said(&h);
    assert!(
        asked.contains("Join this room?"),
        "the link was not asked about at all: {asked}"
    );
    assert!(
        asked.contains("cannot be taken back"),
        "the question does not say what joining costs: {asked}"
    );
    assert!(
        took.borrow().is_empty(),
        "the link was acted on before anybody answered: {:?}",
        took.borrow()
    );

    // **No does nothing, and the question goes.**
    h.get_by_label("No").click();
    h.run();
    h.run();
    assert!(
        took.borrow().is_empty(),
        "No acted on the link anyway: {:?}",
        took.borrow()
    );
    assert!(
        !said(&h).contains("Join this room?"),
        "the question is still on screen after No"
    );

    // **Yes reaches the app**, and the link it gets is the one that was
    // offered rather than a re-parse of something else.
    let url = format!("sigil://contact/{}", key());
    sigil_platform::deeplink::offer(&url).expect("a good link");
    let (mut h, took) = shell_with();
    h.run();
    h.run();
    assert!(
        said(&h).contains("Add"),
        "the contact link was not asked about: {}",
        said(&h)
    );
    h.get_by_label("Yes").click();
    h.run();
    h.run();
    assert_eq!(
        took.borrow().len(),
        1,
        "Yes did not reach the app: {:?}",
        took.borrow()
    );
    assert!(
        matches!(took.borrow()[0], sigil::Link::Contact(_)),
        "the app was handed some other link: {:?}",
        took.borrow()
    );
}
