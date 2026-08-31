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
        let hidden = !ctx.input(|i| i.viewport().focused.unwrap_or(true));
        self.shell.update_all(ctx, hidden);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.shell.ui(ui);
    }
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

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "sigil=info,sigil_net=info".into()),
        )
        .init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("sigil")
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([420.0, 400.0]),
        ..Default::default()
    };

    eframe::run_native(
        "sigil",
        options,
        Box::new(|cc| {
            theme::install(&cc.egui_ctx, theme::light(), theme::dark());
            let apps: Vec<Box<dyn App>> = vec![
                Box::new(sigil_voice::VoiceApp::new()),
                Box::new(sigil_chat::ChatApp::new()),
            ];
            Ok(Box::new(Sigil {
                shell: Shell::new(apps),
            }))
        }),
    )
}
