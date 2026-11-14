//! Print the Noctalia theme rexia would use, then each change as it happens:
//! `cargo run --example noctalia_probe [-- --watch]`.
fn main() -> anyhow::Result<()> {
    let theme = rexia::noctalia::load();
    report(&theme);
    if std::env::args().any(|a| a == "--watch") {
        let _watcher = rexia::noctalia::watch(&theme, |theme| {
            println!("--- theme changed");
            report(&theme);
        })?;
        std::thread::park();
    }
    Ok(())
}

fn report(theme: &rexia::noctalia::NoctaliaTheme) {
    let p = &theme.palette;
    let s = &theme.shell;
    println!(
        "source: {:?} (shell from running Noctalia: {})",
        theme.source, theme.shell_live
    );
    println!(
        "mode: {}  primary #{:06x}  on_primary #{:06x}  surface #{:06x}  on_surface #{:06x}  outline_variant #{:06x}",
        if p.dark { "dark" } else { "light" },
        p.primary,
        p.on_primary,
        p.surface,
        p.on_surface,
        p.outline_variant
    );
    println!(
        "font: {}  corner_radius_scale: {}  borders: button={} card={} input={} popup={}  popup_shadows: {}  panel border: {} {}px",
        s.font_family,
        s.corner_radius_scale,
        s.button_borders,
        s.card_borders,
        s.input_borders,
        s.popup_borders,
        s.popup_shadows,
        s.panel_border_role,
        s.panel_border_width
    );
    let terminal = p.terminal_colors();
    println!(
        "terminal: fg #{:06x} bg #{:06x} red #{:06x} green #{:06x}",
        terminal.foreground, terminal.background, terminal.ansi[1], terminal.ansi[2]
    );
}
