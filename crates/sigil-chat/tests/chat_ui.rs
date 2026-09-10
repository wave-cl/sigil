//! What the chat app says in the states somebody actually meets.

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use sigil::account::Account;
use sigil::app::{App, AppContext};
use sigil::navigator::Navigator;
use sigil::theme;
use sigil_chat::{ChatApp, ChatState, LinkState};

/// A connected session with nothing in it.
///
/// **Not the absence of a session**, which is a different screen: without this
/// these tests drew the "not connected" pane and believed they were drawing
/// the conversation list, because `state_of` used to fall back to a default
/// `ChatState` and that default said the link was up.
fn harness(account: Account) -> Harness<'static> {
    connected(account, empty())
}

/// Connected, and the exchange has answered: there really is nothing here.
///
/// `synced` is what tells the two empties apart -- "you have no
/// conversations" from "we have not asked yet" -- so a fixture that means the
/// first has to say so. Without it these tests read the loading screen, which
/// is the right screen for the other one.
fn empty() -> ChatState {
    ChatState {
        link: LinkState::Up,
        synced: true,
        ..ChatState::default()
    }
}

/// An identity with no exchange configured, which is what an account sigil
/// cannot connect anywhere for actually looks like.
fn adrift(account: Account) -> Harness<'static> {
    build(account, None)
}

fn connected(account: Account, state: ChatState) -> Harness<'static> {
    build(account, Some(state))
}

fn build(account: Account, state: Option<ChatState>) -> Harness<'static> {
    let mut app = ChatApp::new();
    if let Some(state) = state {
        app.show_state_for_test(state);
    }
    let mut accounts = sigil::accounts::Accounts::of(vec![account]);
    Harness::builder()
        .with_size(egui::vec2(1000.0, 620.0))
        .build_ui(move |ui| {
            let ctx = ui.ctx().clone();
            theme::install(&ctx, theme::light(), theme::dark());
            ctx.set_theme(egui::Theme::Dark);
            let t = sigil::ColorTheme::current(&ctx);
            egui::CentralPanel::default()
                .frame(
                    egui::Frame::NONE
                        .fill(t.surface_primary)
                        .inner_margin(egui::Margin::same(sigil::tokens::SPACING_LG as i8)),
                )
                .show(ui, |ui| {
                    let mut nav = Navigator::default();
                    let mut app_ctx = AppContext {
                        navigator: &mut nav,
                        accounts: &mut accounts,
                        unfocused: false,
                        notify: &sigil::Silent,
                        connections: &Default::default(),
                    };
                    let _ = app.render(&mut app_ctx, ui);
                });
        })
}

/// How many text fields are on screen.
///
/// A form is fields; the way to tell whether one is showing is to count them,
/// not to look for its words. See
/// [`the_column_holds_no_forms_until_one_is_asked_for`].
fn fields(h: &Harness<'static>) -> usize {
    fn walk(node: egui_kittest::Node<'_>, n: &mut usize) {
        // By name rather than against the enum: `kittest` does not re-export
        // accesskit's `Role`, and taking a direct dependency on accesskit to
        // compare one variant would pin a version this crate does not
        // otherwise care about.
        if format!("{:?}", node.accesskit_node().role()) == "TextInput" {
            *n += 1;
        }
        for c in node.children() {
            walk(c, n);
        }
    }
    let mut n = 0;
    walk(h.root(), &mut n);
    n
}

fn text_of(h: &Harness<'static>) -> String {
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

/// An open identity with a **random** key.
///
/// Right for a behaviour test and **wrong for a snapshot**: `generate` mints a
/// fresh key every run, and any view that draws a key in full then renders
/// differently every time. Use [`fixed`] where pixels are compared.
fn unlocked(dir: &std::path::Path) -> Account {
    let path = dir.join("identity");
    sqnr::identity::generate(&path, None).unwrap();
    Account::discover(Some(path))
}

/// An open identity whose key is the same on every run. Touches no filesystem.
fn fixed() -> Account {
    Account::unlocked_for_test([5u8; 32])
}

#[test]
fn a_sealed_identity_cannot_chat_yet_and_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("identity");
    sqnr::identity::generate(&path, Some("open sesame")).unwrap();
    let mut h = harness(Account::discover(Some(path)));
    h.run();
    assert!(text_of(&h).contains("Unlock your identity"));
}

/// The connection light says the word as well as the colour. It matters more
/// here than anywhere: while the link is down, messages do not arrive, and
/// nothing happening looks exactly like nobody writing.
#[test]
fn the_connection_state_is_said_in_words() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness(unlocked(dir.path()));
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("connected") || said.contains("reconnecting") || said.contains("offline"),
        "the link is named, not merely coloured: {said}"
    );
}

/// You can write to somebody who has never written to you, which needs their
/// key -- there is nothing else to look them up by.
///
/// The form is a dialog now, so this opens it. **The label is asserted inside
/// the dialog, not on the screen behind it**: an earlier version of this test
/// checked the whole screen for "Write to" and went on passing after the field
/// was removed, because the empty state happens to use the same two words.
#[test]
fn somebody_can_be_added_by_key() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness(unlocked(dir.path()));
    h.run();
    // The property, not the wording: an empty list has to say *both* that it
    // is empty and what to do about it. A bare "nothing here" leaves somebody
    // looking for a control they have not found.
    let said = text_of(&h);
    assert!(
        said.contains("No conversations yet"),
        "an empty list says it is empty: {said}"
    );
    assert!(
        said.contains("key"),
        "and says what to do about it, which needs their key: {said}"
    );

    h.get_by_label("New conversation").click();
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("Write to"),
        "the field is labelled, not only hinted: {said}"
    );
    assert!(said.contains("Add"), "{said}");
}

/// The column holds no forms.
///
/// Writing to somebody, editing your profile and adding an exchange were all
/// inline fields in the conversation list, and the list moved down to make
/// room for them. They are dialogs now.
///
/// # Why this counts fields rather than looking for words
///
/// The first version of this test asserted the *hint* text was absent, and
/// passed for a reason that had nothing to do with the change: a hint never
/// reaches the accessibility tree at all, so it was absent before and after.
/// Asserting the labels was no better -- "Write to" is also in the sentence
/// the empty list says. Counting the text fields measures the thing itself.
#[test]
fn the_column_holds_no_forms_until_one_is_asked_for() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness(unlocked(dir.path()));
    h.run();
    // One: the search box. Searching is not a form -- it filters what is
    // already on screen, so it belongs beside the thing it filters.
    assert_eq!(
        fields(&h),
        1,
        "a form is in the column at rest: {}",
        text_of(&h)
    );
    assert!(text_of(&h).contains("Search"), "{}", text_of(&h));

    h.get_by_label("New conversation").click();
    h.run();
    assert!(
        fields(&h) > 1,
        "asking for the form produced no field: {}",
        text_of(&h)
    );
}

/// An identity with no exchange says so, instead of looking connected.
///
/// `state_of` falls back to a default `ChatState` when there is no session,
/// and every control then talks to a session that does not exist: the command
/// is dropped, the list is empty because there is nothing to list, and the
/// light said **connected**, because that was `LinkState`'s default. Nothing
/// anybody typed did anything and nothing said why.
#[test]
fn an_identity_with_nowhere_to_connect_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = adrift(unlocked(dir.path()));
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("Not connected"),
        "an identity with no exchange claims to be connected: {said}"
    );
    assert!(
        said.contains("names no exchange"),
        "and does not say why: {said}"
    );
    // And it offers the way out rather than a dead screen.
    assert!(said.contains("Add an exchange"), "{said}");
    // The controls of a session that does not exist are not drawn: every one
    // of them would talk to nothing.
    assert!(
        !said.contains("Search"),
        "the conversation list is offered by a session that does not exist: {said}"
    );
}

/// Adding an exchange from the "not connected" pane actually adds one.
#[test]
fn an_exchange_can_be_added_from_the_pane_that_offers_it() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = adrift(unlocked(dir.path()));
    h.run();

    h.get_by_label("Add an exchange").click();
    h.run();
    assert!(
        text_of(&h).contains("a domain, or host:port") || text_of(&h).contains("Exchange"),
        "the dialog did not open: {}",
        text_of(&h)
    );

    // Type into it, the way somebody would.
    // The only field on this screen. Matched on the node's own role name
    // rather than on accesskit's `Role`, which `kittest` does not re-export.
    let field = h.get(
        egui_kittest::kittest::by()
            .predicate(|n| matches!(format!("{:?}", n.role()).as_str(), "TextInput")),
    );
    field.focus();
    field.type_text("indra.org");
    h.run();
    h.get_by_label("Add").click();
    h.run();
    h.run();

    // The pane itself does not list exchanges, so this asks the identity
    // menu, which does — and which is on this screen because the bar is.
    h.get_by_label("Your identity").click();
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("indra.org"),
        "the exchange was not added: {said}"
    );
}

/// An exchange that cannot be added says why, rather than doing nothing.
///
/// `add_exchange` answers `false` for an empty name and for one already held,
/// and both were dropped on the floor: the dialog stayed open with the text
/// still in it and nothing said what had happened.
#[test]
fn an_exchange_that_cannot_be_added_says_why() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = adrift(unlocked(dir.path()));
    h.run();
    h.get_by_label("Add an exchange").click();
    h.run();

    // Nothing typed.
    h.get_by_label("Add").click();
    h.run();
    assert!(
        text_of(&h).contains("Name an exchange"),
        "an empty name did nothing at all: {}",
        text_of(&h)
    );

    // And one already held.
    let field = h.get(
        egui_kittest::kittest::by()
            .predicate(|n| matches!(format!("{:?}", n.role()).as_str(), "TextInput")),
    );
    field.focus();
    field.type_text("indra.org");
    h.run();
    h.get_by_label("Add").click();
    h.run();
    h.get_by_label("Add an exchange").click();
    h.run();
    let field = h.get(
        egui_kittest::kittest::by()
            .predicate(|n| matches!(format!("{:?}", n.role()).as_str(), "TextInput")),
    );
    field.focus();
    field.type_text("indra.org");
    h.run();
    h.get_by_label("Add").click();
    h.run();
    assert!(
        text_of(&h).contains("already connected to indra.org"),
        "adding the same exchange twice did nothing at all: {}",
        text_of(&h)
    );
}

/// A key that is not a key is refused where it was typed, rather than swallowed.
/// An empty list says which empty it is.
///
/// "No conversations yet" is a claim about somebody's account. It was being
/// made on every launch, during the second or two before the exchange answered
/// -- to people whose conversations were about to appear underneath it, and
/// beside a button offering to start their first one.
#[test]
fn a_list_nobody_has_answered_about_yet_says_it_is_loading() {
    let dir = tempfile::tempdir().unwrap();
    let waiting = ChatState {
        link: LinkState::Up,
        synced: false,
        ..ChatState::default()
    };
    let mut h = connected(unlocked(dir.path()), waiting);
    // Stepped, not run: the loading mark asks for the next *step* of itself,
    // and `run` waits for the interface to stop asking for anything at all.
    h.run_steps(3);
    let said = text_of(&h);
    assert!(
        said.contains("Loading your chats"),
        "nothing says the list is still coming: {said}"
    );
    assert!(
        !said.contains("No conversations yet"),
        "an account with conversations in it was told it has none: {said}"
    );

    // And once it has been answered for, the empty really is empty. Its own
    // folder: `unlocked` writes an identity and `generate` refuses to
    // overwrite one, which is the refusal that belongs there.
    let other = tempfile::tempdir().unwrap();
    let mut h = harness(unlocked(other.path()));
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("No conversations yet"),
        "an answered, empty list says nothing at all: {said}"
    );
    assert!(!said.contains("Loading your chats"), "{said}");
}

/// An empty pane must not point at a column that is not on screen.
///
/// The list is there on arriving, so "pick a conversation" names something
/// somebody can see. Put it away and the same pane has to stop naming it and
/// offer it instead — the sentence follows the screen, not the other way
/// round.
#[test]
fn an_empty_pane_names_the_list_only_while_it_is_on_screen() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness(unlocked(dir.path()));
    h.run();
    let said = text_of(&h);
    assert!(
        said.contains("Chats"),
        "signing in did not land on the chats: {said}"
    );
    assert!(
        said.contains("Pick a conversation"),
        "the list is right there and the pane does not say so: {said}"
    );

    h.get_by_label("Hide the chats").click();
    h.run();
    let said = text_of(&h);
    assert!(
        !said.contains("Pick a conversation"),
        "told to pick from a list that is not there: {said}"
    );
    assert!(
        said.contains("Show chats"),
        "and offered no way to one: {said}"
    );
    h.get_by_label("Show chats").click();
    h.run();
    assert!(
        text_of(&h).contains("Chats"),
        "the offer did nothing: {}",
        text_of(&h)
    );
}

#[test]
fn a_bad_key_is_refused_in_place() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness(unlocked(dir.path()));
    h.run();
    h.get_by_label("New conversation").click();
    h.run();
    // Nothing has been typed, so the empty field is not a key.
    h.get_by_label("Add").click();
    h.run();
    assert!(text_of(&h).contains("not a key"), "{}", text_of(&h));
}

#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn chat_dark() {
    // A fixed key, because this pane draws one in full and a generated key
    // makes a snapshot that can never pass twice.
    let mut h = harness(fixed());
    h.run();
    h.snapshot("chat_dark");
}

/// The dialog, so somebody can look at its padding and its margins.
///
/// A new surface that nothing renders is a surface nobody has seen. This is
/// the one form the column no longer holds.
#[test]
#[ignore = "needs a renderer; run via scripts/snapshot-test"]
fn chat_dialog_dark() {
    let mut h = harness(fixed());
    h.run();
    h.get_by_label("New conversation").click();
    h.run();
    h.snapshot("chat_dialog_dark");
}

/// Drawing the same state twice must produce the same thing.
///
/// # Why this exists
///
/// `chat_dark` began failing the moment a view drew the account key in full,
/// because its identity was **generated** — a fresh key every run, so the
/// snapshot could never pass twice. It was not caught locally because
/// `snapshot-test --update` followed by one verify only proves the file that
/// was just written matches the run that wrote it.
///
/// This is the cheap general form of that check: no renderer, no PNG, no
/// platform. Anything non-deterministic that reaches the screen — a generated
/// key, a live clock, a map iterated in hash order — shows up here as two
/// different readings of the same state, in the test that names the problem
/// rather than in a pixel diff on CI.
#[test]
fn the_same_state_draws_the_same_way_twice() {
    let read = || {
        let mut h = harness(fixed());
        h.run();
        text_of(&h)
    };
    assert_eq!(
        read(),
        read(),
        "something drawn here changes between runs, so no snapshot of it can pass twice"
    );
}
