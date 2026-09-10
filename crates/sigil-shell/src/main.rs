//! sigil — voice and chat for sqex, in one window.

use sigil::app::App;
use sigil::theme;

use sigil_shell::Shell;

/// The host. Only this implements `eframe::App`; the shell is simply the thing
/// it draws.
///
/// eframe 0.36 splits an app into `logic` and `ui`, and `logic` keeps running
/// **while the window is hidden** — with no egui pass at all. That is what lets
/// sigil hold a call and listen for a ring while closed to the tray, so the
/// split is load-bearing rather than tidiness.
struct Sigil {
    shell: Shell,
}

impl eframe::App for Sigil {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let unfocused = !ctx.input(|i| i.viewport().focused.unwrap_or(true));
        self.shell.update_all(ctx, unfocused);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.shell.set_top_inset(chrome_inset(ui.ctx(), frame));
        self.shell.ui(ui);
    }
}

/// How far down the window sigil's own content has to start.
///
/// The title bar is transparent and the content is drawn behind it (see the
/// viewport below), so the close, minimise and zoom buttons sit **over** the
/// top-left of whatever is there. This is how much room they need.
///
/// Measured from the window rather than written down as a number: the buttons
/// are the system's, their size is a system decision, and a constant here
/// would be a guess that goes wrong quietly on the day it changes. Divided by
/// the zoom factor because the metrics come back in the window's own scale and
/// everything in egui is in points.
#[cfg(target_os = "macos")]
fn chrome_inset(ctx: &egui::Context, frame: &eframe::Frame) -> f32 {
    use raw_window_handle::HasWindowHandle as _;

    let Ok(handle) = frame.window_handle() else {
        // No window to ask. Anything drawn now is drawn behind the buttons,
        // but there is nothing better to say than "no room needed".
        return 0.0;
    };
    eframe::WindowChromeMetrics::from_window_handle(&handle.as_raw())
        .map(|m| m.traffic_lights_size.y / ctx.zoom_factor())
        .unwrap_or(0.0)
}

/// Everywhere else the desktop draws its own title bar above the window, and
/// nothing of sigil's is underneath it.
#[cfg(not(target_os = "macos"))]
fn chrome_inset(_ctx: &egui::Context, _frame: &eframe::Frame) -> f32 {
    0.0
}

fn main() -> eframe::Result<()> {
    // A multi-thread runtime, entered for the life of the process rather than
    // blocked on. eframe owns the main thread and the event loop; the runtime
    // exists so that a call spawned from a button click has an executor to run
    // on, and keeps running while the interface goes on drawing.
    //
    // Held in `main` deliberately: dropping a runtime waits for its tasks, and
    // doing that in a destructor somewhere down the tree would mean a hang on
    // quit with no obvious cause.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("build the tokio runtime");
    let _guard = runtime.enter();

    // One sigil at a time, claimed before anything else opens. sqex-chat
    // flocks the account store anyway, so a second instance would fail later
    // and less clearly; this turns that into a refusal with a pid in it.
    let data = dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("sigil");
    let _instance = match sigil_platform::instance::claim(&data) {
        Ok(i) => i,
        Err(why) => {
            eprintln!("sigil: {why}");
            // A second launch is usually somebody trying to bring the running
            // one forward. Saying so beats a bare exit code.
            eprintln!("       the running sigil holds this account's store.");
            std::process::exit(1);
        }
    };

    // A link on the command line is an *offer*. Parsed here so a malformed one
    // is refused before a window opens, and never acted on: `sigil://room/...`
    // joining silently would put somebody in a conversation they did not
    // choose, which cannot be undone because membership is holding the secret.
    let offered = std::env::args().nth(1).and_then(|arg| {
        if !arg.starts_with("sigil://") {
            return None;
        }
        match sigil_platform::deeplink::parse(&arg) {
            Ok(link) => Some(link),
            Err(why) => {
                eprintln!("sigil: {why}");
                None
            }
        }
    });
    if let Some(link) = &offered {
        tracing::info!(
            "opened with a link, awaiting confirmation: {}",
            sigil_platform::deeplink::confirmation(link)
        );
    }

    tracing_subscriber::fmt()
        // stderr, not stdout. A window has no console to read, so these are
        // for whoever ran sigil from one or is reading a redirect -- and stdout
        // through a pipe is block-buffered, so a killed process loses
        // everything it had to say. That is how the first attempt at this
        // produced an empty log.
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "sigil=info,sigil_net=info".into()),
        )
        .init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            // Kept, and not shown: the window manager, the dock and every
            // window list still want a name. What nobody wants is the word
            // "sigil" written across the top of sigil.
            .with_title("sigil")
            .with_title_shown(false)
            // The bar goes transparent and the content is drawn behind it, so
            // the top of the window is the application's own colour instead of
            // the system's grey. macOS only; on the others the desktop draws
            // the bar and these are ignored.
            //
            // **The buttons stay.** `titlebar_shown(false)` makes the bar
            // transparent rather than removing it, so close, minimise and zoom
            // are where they always are and the bar still drags the window --
            // which is why this is not `decorations(false)`, where sigil would
            // have to draw and drag all three itself.
            .with_titlebar_shown(false)
            .with_fullsize_content_view(true)
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([420.0, 400.0]),
        ..Default::default()
    };

    eframe::run_native(
        "sigil",
        options,
        Box::new(|cc| {
            theme::install(&cc.egui_ctx, theme::light(), theme::dark());
            // Without this every image attachment is a broken-picture icon:
            // `egui::Image` decodes nothing on its own. See `install_loaders`.
            sigil_ui::install_loaders(&cc.egui_ctx);

            // Built here, and only here: the tray must be created on the main
            // thread — the macOS menu bar and Linux's GTK context both insist —
            // and eframe's creator is the one place that is guaranteed to be.
            let platform = sigil_platform::Platform::new();

            // Said at startup as well as drawn in the Desktop pane. The first
            // question about a notification that never appeared is whether it
            // was ever possible, and an answer in the log is one somebody can
            // paste into a bug report.
            for capability in platform.capabilities() {
                match capability.support.reason() {
                    None => tracing::info!("{}: available", capability.name),
                    Some(why) => tracing::warn!("{}: unavailable — {why}", capability.name),
                }
            }
            if !platform.can_reach_you_when_away() {
                // Worth saying loudly rather than letting somebody find out by
                // missing a call: with neither notifications nor a tray, sigil
                // is a telephone only while its window is open.
                tracing::warn!(
                    "no notifications and no tray on this desktop: calls will only \
                     reach you while sigil's window is open"
                );
            }

            // No Calls tab. A call is placed from inside the conversation
            // with the person you are calling, rings there, and carries its
            // audio there -- so a separate app for it was a second place to
            // look for something that had already happened somewhere else.
            // `sigil-voice` still holds the engine and the call views.
            let apps: Vec<Box<dyn App>> = vec![
                Box::new(sigil_chat::ChatApp::new()),
                Box::new(sigil_admin::AdminApp::new()),
                Box::new(sigil_shell::PlatformApp::new(
                    sigil_platform::Platform::new(),
                )),
            ];
            let shell = Shell::new(apps, Some(platform));

            // Which identities were re-opened, and what state each is in.
            //
            // The roster used to be one identity read from a fixed path; it is
            // now runtime state restored from a file, and state that came from
            // a file with nothing announcing it leaves nobody able to say what
            // the program thinks it is holding. "Sigil is not ringing" and
            // "sigil is not holding that account at all" look identical from
            // outside and want completely different fixes.
            let accounts = shell.accounts();
            tracing::info!(
                "holding {} identit{}",
                accounts.len(),
                if accounts.len() == 1 { "y" } else { "ies" }
            );
            for (i, held) in accounts.all().enumerate() {
                let account = held.account();
                let shown = if i == accounts.active_index() {
                    " (shown)"
                } else {
                    ""
                };
                match account.unlocked() {
                    Some(u) => tracing::info!("  {}{shown} — open", u.me()),
                    None => tracing::info!(
                        "  {}{shown} — {}",
                        account.path().display(),
                        account.describe()
                    ),
                }
                // Which exchanges, by name. This is remembered state read back
                // from a file, and state that came from a file with nothing
                // announcing it leaves nobody able to say what the program
                // thinks it is holding.
                for exchange in held.exchanges() {
                    match exchange.as_str() {
                        "" => tracing::info!("      at the exchange this identity names"),
                        named => tracing::info!("      at {named}"),
                    }
                }
            }

            Ok(Box::new(Sigil { shell }))
        }),
    )
}
