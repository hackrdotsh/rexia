//! Start rexia: read settings and the Noctalia theme, then open the workspace window.
use crate::{
    noctalia,
    notify::Notifier,
    settings::{self, HostConfig, Settings},
    ui::{icons::Assets, theme::Theme, workspace::Workspace},
};
use clap::Parser;
use gpui::{
    App, AppContext as _, Bounds, TitlebarOptions, WindowBounds, WindowDecorations, WindowOptions,
    px, size,
};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(version, about = "A Rex client for Linux, themed by Noctalia")]
pub struct Options {
    /// Connect to this server instead of the local one, as `rex -S` takes it.
    #[arg(long, short = 'S')]
    pub server: Option<String>,
    /// Show this session first, by ID.
    #[arg(long, short = 's')]
    pub session: Option<String>,
    /// Use this settings file instead of $XDG_CONFIG_HOME/rexia/settings.toml.
    #[arg(long)]
    pub config: Option<PathBuf>,
    /// Print the Noctalia template registration for rexia's palette, then exit.
    #[arg(long)]
    pub noctalia_template: bool,
}

fn template_path() -> PathBuf {
    let installed = std::env::current_exe().ok().and_then(|exe| {
        Some(
            exe.parent()?
                .parent()?
                .join("share/rexia/noctalia/rexia.toml"),
        )
    });
    installed.filter(|p| p.exists()).unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/noctalia/rexia.toml")
    })
}

pub fn run() -> anyhow::Result<()> {
    let options = Options::parse();
    if options.noctalia_template {
        print!("{}", noctalia::template_registration(&template_path()));
        return Ok(());
    }
    let path = options
        .config
        .clone()
        .unwrap_or_else(settings::default_path);
    let mut settings = Settings::load(&path)?;
    let save_path = if let Some(server) = &options.server {
        // A one-off server replaces the host list for this run and isn't saved.
        settings.hosts = vec![HostConfig {
            id: "cli".into(),
            label: server.clone(),
            endpoint: server.clone(),
        }];
        settings.last_host = None;
        None
    } else {
        Some(path)
    };
    if let Some(session) = options.session.clone() {
        settings.last_session = Some(session);
    }

    let platform = gpui_platform::current_platform(false);
    let font_metrics = crate::ui::fonts::TerminalFontMetrics(platform.text_system());
    gpui::Application::with_platform(platform)
        .with_assets(Assets)
        .run(move |cx: &mut App| {
            cx.set_global(Theme::from_noctalia(&noctalia::load()));
            cx.set_global(font_metrics);
            // rexia quits when its last window closes.
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            let opened = open_workspace(settings, save_path, cx, |workspace, window, cx| {
                // The first window sends desktop notifications for all of them.
                let (notifier, clicks) = Notifier::spawn(None);
                workspace.set_notifier(notifier, clicks, window, cx);
            });
            if let Err(error) = opened {
                eprintln!("rexia: couldn't open a window: {error:#}");
                cx.quit();
            }
        });
    Ok(())
}

/// Open a workspace window, letting `setup` finish the workspace before it first draws.
pub fn open_workspace(
    settings: Settings,
    save_path: Option<PathBuf>,
    cx: &mut App,
    setup: impl FnOnce(&mut Workspace, &mut gpui::Window, &mut gpui::Context<Workspace>) + 'static,
) -> anyhow::Result<gpui::WindowHandle<Workspace>> {
    let bounds = Bounds::centered(None, size(px(1280.), px(800.)), cx);
    let window_background = if settings.interface.translucent_panes() {
        gpui::WindowBackgroundAppearance::Blurred
    } else {
        gpui::WindowBackgroundAppearance::Opaque
    };
    cx.open_window(
        WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            window_background,
            titlebar: Some(TitlebarOptions {
                title: Some("rexia".into()),
                ..Default::default()
            }),
            app_id: Some("dev.hackr.rexia".into()),
            window_decorations: Some(WindowDecorations::Server),
            window_min_size: Some(size(px(480.), px(320.))),
            ..Default::default()
        },
        |window, cx| {
            cx.new(|cx| {
                let mut workspace = Workspace::new(settings, save_path, true, window, cx);
                setup(&mut workspace, window, cx);
                workspace
            })
        },
    )
}
