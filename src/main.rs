//! FOLD: your files, in flow.
//!
//! Module map (read in this order if you're new to the code):
//!   main.rs   start-up, the event loop, and cleanup
//!   cli.rs    command-line arguments
//!   theme.rs  colours (Spicetify colour files, contrast checks, fallback)
//!   dir.rs    reading folders: sorting, hidden files, filtering
//!   fsops.rs  copy / move / rename / delete, with the safety rules
//!   input.rs  which key means which action
//!   app.rs    the state machine that ties it all together
//!   ui.rs     drawing with Ratatui

mod app;
mod cli;
mod dir;
mod fsops;
mod input;
mod kitty;
mod theme;
mod ui;

use crossterm::event::{self, Event, KeyEventKind};
use ratatui::DefaultTerminal;
use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
use std::io::{self, IsTerminal};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("fold: {message}");
            ExitCode::FAILURE
        }
    }
}

/// Plain ASCII when the locale isn't UTF-8 or the terminal is the bare Linux
/// console, or when the user asks for it with FOLD_ASCII=1.
fn detect_unicode() -> bool {
    if let Ok(v) = std::env::var("FOLD_ASCII") {
        if !v.is_empty() && v != "0" {
            return false;
        }
    }
    if std::env::var("TERM")
        .map(|t| t == "linux" || t == "dumb")
        .unwrap_or(false)
    {
        return false;
    }
    let locale = ["LC_ALL", "LC_CTYPE", "LANG"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .find(|v| !v.is_empty())
        .unwrap_or_default()
        .to_ascii_lowercase();
    locale.contains("utf-8") || locale.contains("utf8")
}

fn run() -> Result<(), String> {
    let cli = cli::parse(std::env::args_os().skip(1))?;
    if cli.help {
        print!("{}", cli::HELP);
        return Ok(());
    }
    if cli.version {
        println!("fold {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(
            "FOLD needs an interactive terminal (stdin/stdout must not be redirected)".into(),
        );
    }

    let start = match cli.start {
        Some(p) => p,
        None => std::env::current_dir()
            .map_err(|e| format!("cannot determine current directory: {e}"))?,
    };

    // Load everything that can fail *before* touching the terminal, so error
    // messages print normally.
    let explicit_theme = cli.theme.is_some()
        || cli.matugen_image.is_some()
        || std::env::var_os("FOLD_THEME").is_some_and(|value| !value.is_empty());
    let loaded = if let Some(image) = cli.matugen_image {
        theme::load_matugen(&image)
    } else {
        theme::load(cli.theme)
    };
    let command_theme = loaded.theme.clone();
    let mut app = app::App::new(&start, loaded.theme, detect_unicode())?;
    if explicit_theme {
        app.theme = command_theme;
        app.theme_choice = None;
    }
    if let Some(first) = loaded.warnings.first() {
        app.warn(first.clone());
    }

    // SIGTERM / SIGHUP / SIGINT set a flag; the loop notices and exits cleanly.
    let stop = Arc::new(AtomicBool::new(false));
    for signal in [SIGTERM, SIGHUP, SIGINT] {
        signal_hook::flag::register(signal, Arc::clone(&stop))
            .map_err(|e| format!("cannot install signal handler: {e}"))?;
    }

    // `ratatui::init` switches to the alternate screen, enables raw mode and
    // installs a panic hook that restores the terminal.
    let mut terminal = ratatui::init();
    let result = event_loop(&mut terminal, &mut app, &stop);
    // Always restore, whether the loop ended normally or with an error.
    ratatui::restore();
    result.map_err(|e| format!("terminal error: {e}"))
}

fn event_loop(
    terminal: &mut DefaultTerminal,
    app: &mut app::App,
    stop: &AtomicBool,
) -> io::Result<()> {
    let mut dirty = true;
    let mut kitty_image_visible = false;
    while !app.should_quit && !stop.load(Ordering::Relaxed) {
        if dirty {
            let showing_kitty_image =
                kitty::supported() && matches!(&app.mode, app::Mode::ImageViewer { .. });
            if kitty_image_visible && !showing_kitty_image {
                kitty::clear()?;
                kitty_image_visible = false;
            }
            terminal.draw(|frame| ui::draw(frame, app))?;
            if showing_kitty_image {
                if let app::Mode::ImageViewer { image, .. } = &app.mode {
                    let area = terminal.size()?;
                    kitty::show(image, area.width, area.height)?;
                    kitty_image_visible = true;
                }
            }
            dirty = false;
        }
        // Wake up a few times a second so we notice signals.
        if event::poll(Duration::from_millis(250))? {
            match event::read()? {
                Event::Key(key) if key.kind != KeyEventKind::Release => {
                    app.on_key(key);
                    dirty = true;
                }
                Event::Resize(..) => dirty = true,
                _ => {}
            }
        }
    }
    if kitty_image_visible {
        kitty::clear()?;
    }
    Ok(())
}
