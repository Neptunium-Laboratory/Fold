//! Rendering. This file only *reads* `App` (apart from telling it how tall the
//! list is, so scrolling works) and paints it with Ratatui.

use crate::app::{App, Confirm, Level, Mode};
use crate::dir::{human_size, sanitize};
use crate::fsops::display_name;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::symbols::border;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Cell, Clear, Paragraph, Row, Table, Wrap};
use ratatui::Frame;

/// Characters used for the little bits of decoration, with a plain-ASCII set
/// for terminals that can't show box drawing characters.
struct Glyphs {
    mark: &'static str,
    cursor: &'static str,
    selected: &'static str,
    ellipsis: &'static str,
    border: border::Set,
}

fn glyphs(unicode: bool) -> Glyphs {
    if unicode {
        Glyphs {
            mark: "┌─ FOLD",
            cursor: "▌",
            selected: "●",
            ellipsis: "…",
            border: border::PLAIN,
        }
    } else {
        Glyphs {
            mark: "+- FOLD",
            cursor: ">",
            selected: "*",
            ellipsis: "..",
            border: border::Set {
                top_left: "+",
                top_right: "+",
                bottom_left: "+",
                bottom_right: "+",
                vertical_left: "|",
                vertical_right: "|",
                horizontal_top: "-",
                horizontal_bottom: "-",
            },
        }
    }
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .areas(area);

    // The list needs to know its height so the cursor can stay on screen.
    let inner_height = body.height.saturating_sub(2) as usize;
    app.ensure_cursor_visible(inner_height);
    let app: &App = app;

    let t = &app.theme;
    let g = glyphs(app.unicode);
    f.render_widget(Block::new().style(Style::new().bg(t.bg).fg(t.fg)), area);

    draw_header(f, app, &g, header);
    draw_list(f, app, &g, body);
    draw_footer(f, app, footer);

    match &app.mode {
        Mode::Confirm(c) => draw_confirm(f, app, c, area),
        Mode::Rename { input, error, .. } => {
            let rect = centered(area, 56, 6);
            let block = dialog_block(app, " Rename ", t.accent, &g);
            f.render_widget(Clear, rect);
            let inner = block.inner(rect);
            f.render_widget(block, rect);
            let mut lines = vec![
                Line::styled("New name:", Style::new().fg(t.muted)),
                Line::styled(input.value(), Style::new().fg(t.fg)),
                Line::raw(""),
            ];
            lines.push(match error {
                Some(e) => Line::styled(e.clone(), Style::new().fg(t.error)),
                None => hint_line(app, &[("enter", "rename"), ("esc", "cancel")]),
            });
            f.render_widget(Paragraph::new(lines), inner);
            let x = inner.x + Line::raw(input.before_cursor()).width() as u16;
            f.set_cursor_position((x.min(inner.right().saturating_sub(1)), inner.y + 1));
        }
        Mode::Message { title, lines } => {
            let mut body: Vec<Line> = lines
                .iter()
                .take(10)
                .map(|l| Line::styled(sanitize(l), Style::new().fg(t.fg)))
                .collect();
            if lines.len() > 10 {
                body.push(Line::styled(
                    format!("… and {} more", lines.len() - 10),
                    Style::new().fg(t.muted),
                ));
            }
            body.push(Line::raw(""));
            body.push(Line::styled("press any key", Style::new().fg(t.muted)));
            let h = body.len() as u16 + 2;
            let rect = centered(area, 70, h);
            let block = dialog_block(app, &format!(" {title} "), t.error, &g);
            f.render_widget(Clear, rect);
            let inner = block.inner(rect);
            f.render_widget(block, rect);
            f.render_widget(Paragraph::new(body).wrap(Wrap { trim: false }), inner);
        }
        Mode::Help => draw_help(f, app, &g, area),
        Mode::Settings { selected } => draw_settings(f, app, &g, area, *selected),
        Mode::Bookmarks { selected } => draw_bookmarks(f, app, &g, area, *selected),
        Mode::TextViewer {
            title,
            lines,
            offset,
        } => draw_text_viewer(f, app, title, lines, *offset, area, &g),
        Mode::ImageViewer { title, image } => draw_image_viewer(f, app, title, image, area, &g),
        Mode::Normal | Mode::Filter(_) => {}
    }
}

fn draw_settings(f: &mut Frame, app: &App, g: &Glyphs, area: Rect, selected: usize) {
    let t = &app.theme;
    let rect = centered(area, 64, 22);
    let block = dialog_block(app, " Settings ", t.accent, g);
    f.render_widget(Clear, rect);
    let inner = block.inner(rect);
    f.render_widget(block, rect);
    let logo = [
        "███████╗ ██████╗ ██╗     ██████╗",
        "██╔════╝██╔═══██╗██║     ██╔══██╗",
        "█████╗  ██║   ██║██║     ██║  ██║",
        "██╔══╝  ██║   ██║██║     ██║  ██║",
        "██║     ╚██████╔╝███████╗██████╔╝",
        "╚═╝      ╚═════╝ ╚══════╝╚═════╝",
    ];
    let mut lines: Vec<Line> = logo
        .iter()
        .map(|line| Line::styled(*line, Style::new().fg(t.accent)))
        .collect();
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "Fast Organized Links & Directories",
        Style::new().fg(t.fg),
    ));
    lines.push(Line::styled(
        format!("Version {}", env!("CARGO_PKG_VERSION")),
        Style::new().fg(t.muted),
    ));
    lines.push(Line::styled("Made by Neptunium", Style::new().fg(t.muted)));
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "Theme",
        Style::new().fg(t.fg).add_modifier(Modifier::BOLD),
    ));
    let choices = [
        "Catppuccin Mocha",
        "Osaka Jane",
        "One Dark",
        "Gruvbox",
        "Matugen / terminal colors",
    ];
    for (index, choice) in choices.iter().enumerate() {
        let is_selected = index == selected;
        let active = app.theme_choice == Some(index);
        let marker = if is_selected { "› " } else { "  " };
        let suffix = if active { "  ✓ active" } else { "" };
        let style = if is_selected {
            Style::new()
                .fg(t.cursor_fg)
                .bg(t.cursor_bg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(t.fg)
        };
        lines.push(Line::styled(format!("{marker}{choice}{suffix}"), style));
    }
    lines.push(Line::styled(
        format!("Current palette: {}", app.theme.source),
        Style::new().fg(t.muted),
    ));
    let icon_style = if selected == 5 {
        Style::new()
            .fg(t.cursor_fg)
            .bg(t.cursor_bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(t.fg)
    };
    lines.push(Line::styled(
        format!("File icons: {}", if app.show_icons { "on" } else { "off" }),
        icon_style,
    ));
    lines.push(Line::styled(
        "↑↓ select   enter apply / toggle   esc close",
        Style::new().fg(t.muted),
    ));
    f.render_widget(Paragraph::new(lines).centered(), inner);
}

fn draw_bookmarks(f: &mut Frame, app: &App, g: &Glyphs, area: Rect, selected: usize) {
    let t = &app.theme;
    let height = (app.bookmarks.len().min(14) as u16 + 5).min(area.height);
    let rect = centered(area, 78, height.max(7));
    let block = dialog_block(app, " Bookmarks ", t.accent, g);
    f.render_widget(Clear, rect);
    let inner = block.inner(rect);
    f.render_widget(block, rect);
    let capacity = inner.height.saturating_sub(3) as usize;
    let start = selected.saturating_sub(capacity.saturating_sub(1));
    let mut lines = Vec::new();
    if app.bookmarks.is_empty() {
        lines.push(Line::styled(
            "No bookmarks yet. Press b to save this folder.",
            Style::new().fg(t.muted),
        ));
    } else {
        for (index, path) in app.bookmarks.iter().enumerate().skip(start).take(capacity) {
            let marker = if index == selected { "› " } else { "  " };
            let style = if index == selected {
                Style::new()
                    .fg(t.cursor_fg)
                    .bg(t.cursor_bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::new().fg(t.fg)
            };
            lines.push(Line::styled(
                format!("{marker}{}", sanitize(&path.to_string_lossy())),
                style,
            ));
        }
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        "enter open   d remove   esc close",
        Style::new().fg(t.muted),
    ));
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn draw_text_viewer(
    f: &mut Frame,
    app: &App,
    title: &str,
    lines: &[String],
    offset: usize,
    area: Rect,
    g: &Glyphs,
) {
    let t = &app.theme;
    let rect = centered(
        area,
        area.width.saturating_sub(4),
        area.height.saturating_sub(3),
    );
    let block = dialog_block(app, &format!(" {title} "), t.accent, g);
    f.render_widget(Clear, rect);
    let inner = block.inner(rect);
    f.render_widget(block, rect);
    let visible: Vec<Line> = lines
        .iter()
        .skip(offset)
        .take(inner.height.saturating_sub(1) as usize)
        .enumerate()
        .map(|(i, line)| {
            Line::from(vec![
                Span::styled(
                    format!("{:>4} │ ", offset + i + 1),
                    Style::new().fg(t.accent),
                ),
                Span::styled(line.clone(), Style::new().fg(t.fg)),
            ])
        })
        .collect();
    let mut visible = visible;
    visible.push(Line::styled(
        "↑↓/jk scroll  pgup/pgdn page  q/esc close",
        Style::new().fg(t.muted),
    ));
    f.render_widget(
        Paragraph::new(visible)
            .style(Style::new().bg(t.bg))
            .wrap(Wrap { trim: false }),
        inner,
    );
}

fn draw_image_viewer(
    f: &mut Frame,
    app: &App,
    title: &str,
    image: &image::DynamicImage,
    area: Rect,
    g: &Glyphs,
) {
    let t = &app.theme;
    let rect = centered(
        area,
        area.width.saturating_sub(4),
        area.height.saturating_sub(3),
    );
    let block = dialog_block(
        app,
        &format!(" {title} · {}×{} ", image.width(), image.height()),
        t.accent,
        g,
    );
    f.render_widget(Clear, rect);
    let inner = block.inner(rect);
    f.render_widget(block, rect);
    if inner.width == 0 || inner.height < 2 {
        return;
    }
    let cols = inner.width as u32;
    let rows = inner.height.saturating_sub(1) as u32;
    // Each terminal cell carries two full-color samples using the upper-half
    // block glyph. Unlike Braille's on/off dots, this preserves smooth color
    // gradients and avoids making photographs look stippled.
    let pixel_width = cols;
    let pixel_height = rows.saturating_mul(2);
    let ratio = (image.width() as f64 / pixel_width as f64)
        .max(image.height() as f64 / pixel_height as f64)
        .max(1.0);
    let w = ((image.width() as f64 / ratio).round() as u32)
        .max(1)
        .min(pixel_width);
    let h = ((image.height() as f64 / ratio).round() as u32)
        .max(1)
        .min(pixel_height);
    let resized = image
        // The preview is usually much smaller than the source. A high-quality
        // downsampling filter preserves detail and avoids the blocky look of
        // nearest-neighbor scaling on photographs.
        .resize_exact(w, h, image::imageops::FilterType::Lanczos3)
        .to_rgb8();
    let cell_width = w;
    let cell_height = (h + 1) / 2;
    let x0 = inner.x + (inner.width - cell_width as u16) / 2;
    let y0 = inner.y + (inner.height.saturating_sub(1) - cell_height as u16) / 2;
    for cy in 0..cell_height {
        let spans: Vec<Span> = (0..cell_width)
            .map(|x| {
                let top = resized.get_pixel(x, cy * 2).0;
                let bottom = if cy * 2 + 1 < h {
                    resized.get_pixel(x, cy * 2 + 1).0
                } else {
                    top
                };
                Span::styled(
                    "▀",
                    Style::new()
                        .fg(ratatui::style::Color::Rgb(top[0], top[1], top[2]))
                        .bg(ratatui::style::Color::Rgb(bottom[0], bottom[1], bottom[2])),
                )
            })
            .collect();
        f.render_widget(
            Paragraph::new(Line::from(spans)),
            Rect::new(x0, y0 + cy as u16, cell_width as u16, 1),
        );
    }
    f.render_widget(
        Paragraph::new(Line::styled("q / esc close", Style::new().fg(t.muted)).centered()),
        Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1),
    );
}

fn draw_header(f: &mut Frame, app: &App, g: &Glyphs, area: Rect) {
    let t = &app.theme;
    let left = Line::from(vec![
        Span::styled(
            g.mark,
            Style::new().fg(t.accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled("  your files, in flow", Style::new().fg(t.muted)),
    ]);
    f.render_widget(Paragraph::new(left), area);

    let mut chips: Vec<String> = Vec::new();
    if let Some(clip) = &app.clipboard {
        chips.push(format!("{} {}", clip.op.label(), clip.items.len()));
    }
    let marked = app.marked_count();
    if marked > 0 {
        chips.push(format!("{marked} selected"));
    }
    if app.show_hidden {
        chips.push("hidden shown".into());
    }
    if !app.filter.is_empty() {
        chips.push(format!("filter: {}", sanitize(&app.filter)));
    }
    if !chips.is_empty() {
        let right = Line::styled(chips.join("  ·  "), Style::new().fg(t.accent)).right_aligned();
        f.render_widget(Paragraph::new(right), area);
    }
}

/// Shorten a long path from the left: `…/deep/inside/here`.
fn shorten_left(text: &str, max: usize, ellipsis: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= max {
        return text.to_string();
    }
    let keep = max.saturating_sub(ellipsis.chars().count());
    let tail: String = chars[chars.len() - keep..].iter().collect();
    format!("{ellipsis}{tail}")
}

fn draw_list(f: &mut Frame, app: &App, g: &Glyphs, area: Rect) {
    let t = &app.theme;
    let path = sanitize(&app.cwd.to_string_lossy());
    let title = shorten_left(&path, area.width.saturating_sub(6) as usize, g.ellipsis);
    let position = if app.visible.is_empty() {
        " empty ".to_string()
    } else {
        format!(" {}/{} ", app.cursor + 1, app.visible.len())
    };

    let block = Block::bordered()
        .border_set(g.border)
        .border_style(Style::new().fg(t.border))
        .title(Line::styled(format!(" {title} "), Style::new().fg(t.fg)))
        .title_bottom(Line::styled(position, Style::new().fg(t.muted)).right_aligned());
    let inner = block.inner(area);
    f.render_widget(block, area);

    if app.visible.is_empty() {
        let msg = if app.filter.is_empty() {
            "This folder is empty"
        } else {
            "No files match the filter"
        };
        let rect = Rect::new(
            inner.x,
            inner.y + inner.height / 2,
            inner.width,
            1.min(inner.height),
        );
        f.render_widget(
            Paragraph::new(Line::styled(msg, Style::new().fg(t.muted)).centered()),
            rect,
        );
        return;
    }

    let height = inner.height as usize;
    let end = (app.offset + height).min(app.visible.len());
    let rows: Vec<Row> = (app.offset..end).map(|i| row_for(app, g, i)).collect();
    let widths = [
        Constraint::Length(2),
        Constraint::Fill(1),
        Constraint::Length(9),
    ];
    f.render_widget(Table::new(rows, widths).column_spacing(1), inner);
}

fn row_for(app: &App, g: &Glyphs, vis: usize) -> Row<'static> {
    let t = &app.theme;
    let e = &app.entries[app.visible[vis]];
    let is_cursor = vis == app.cursor;
    let marked = app.selected.contains(&e.path);

    let mut name_style = Style::new().fg(if e.hidden { t.muted } else { t.fg });
    if e.is_dir {
        name_style = name_style.add_modifier(Modifier::BOLD);
    }
    if e.is_symlink {
        name_style = name_style.add_modifier(Modifier::ITALIC);
    }
    if marked {
        name_style = name_style.fg(t.accent);
    }
    if is_cursor {
        name_style = name_style
            .fg(if marked { t.accent } else { t.cursor_fg })
            .add_modifier(Modifier::BOLD);
    }

    let mut label = e.display.clone();
    if e.is_dir {
        label.push('/');
    } else if e.is_symlink {
        label.push('@');
    }

    let size = if e.broken_link {
        "broken".to_string()
    } else if e.is_dir {
        String::new()
    } else {
        human_size(e.size)
    };

    let accent = Style::new().fg(t.accent);
    let gutter = Line::from(vec![
        Span::styled(if is_cursor { g.cursor } else { " " }, accent),
        Span::styled(if marked { g.selected } else { " " }, accent),
    ]);

    let name_cell = if app.show_icons {
        let icon = file_icon(e);
        Cell::from(Line::from(vec![
            Span::styled(
                format!("{icon} "),
                Style::new().fg(if e.is_dir { t.accent } else { t.muted }),
            ),
            Span::styled(label, name_style),
        ]))
    } else {
        Cell::from(Line::styled(label, name_style))
    };
    let row = Row::new([
        Cell::from(gutter),
        name_cell,
        Cell::from(Line::styled(size, Style::new().fg(t.muted)).right_aligned()),
    ]);
    if is_cursor {
        row.style(Style::new().bg(t.cursor_bg))
    } else {
        row
    }
}

fn file_icon(entry: &crate::dir::Entry) -> &'static str {
    let ext = entry
        .path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let kind = if entry.is_symlink {
        1
    } else if entry.is_dir {
        0
    } else if matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "tif" | "tiff" | "svg"
    ) {
        2
    } else if matches!(
        ext.as_str(),
        "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" | "rar" | "zst"
    ) {
        3
    } else if matches!(
        ext.as_str(),
        "rs" | "c"
            | "h"
            | "cpp"
            | "hpp"
            | "py"
            | "js"
            | "ts"
            | "jsx"
            | "tsx"
            | "go"
            | "java"
            | "kt"
            | "swift"
            | "sh"
            | "bash"
            | "html"
            | "css"
            | "sql"
    ) {
        4
    } else if matches!(
        ext.as_str(),
        "txt"
            | "md"
            | "markdown"
            | "rst"
            | "pdf"
            | "doc"
            | "docx"
            | "odt"
            | "rtf"
            | "csv"
            | "json"
            | "toml"
            | "yaml"
            | "yml"
            | "ini"
            | "conf"
            | "cfg"
    ) {
        5
    } else {
        6
    };
    // ASCII badges avoid missing-glyph boxes on fonts without icon coverage.
    ["[D]", "[L]", "[I]", "[Z]", "[C]", "[T]", "[F]"][kind]
}

/// Key hints, in priority order. Items are dropped from the end if the
/// terminal is too narrow; the last two (help, quit) always stay.
fn normal_hints(app: &App) -> Vec<(&'static str, &'static str)> {
    let mut v = Vec::new();
    if app.clipboard.is_some() {
        v.push(("p", "paste here"));
        v.push(("esc", "cancel"));
    }
    v.extend([
        ("enter", "open"),
        ("bksp", "up"),
        ("space", "select"),
        ("c", "copy"),
        ("m", "move"),
        ("r", "rename"),
        ("d", "delete"),
        ("/", "filter"),
        ("b", "bookmark current folder"),
        ("B", "open bookmarks"),
        (".", "hidden"),
        ("↑↓", "move"),
    ]);
    v
}

fn hint_spans(app: &App, hints: &[(&str, &str)]) -> Vec<Span<'static>> {
    let t = &app.theme;
    let mut spans = Vec::new();
    for (i, (key, label)) in hints.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(
            key.to_string(),
            Style::new().fg(t.fg).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(format!(" {label}"), Style::new().fg(t.muted)));
    }
    spans
}

fn hint_line(app: &App, hints: &[(&str, &str)]) -> Line<'static> {
    Line::from(hint_spans(app, hints))
}

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let t = &app.theme;

    if let Mode::Filter(input) = &app.mode {
        let prefix = "/ ";
        let line = Line::from(vec![
            Span::styled(
                prefix,
                Style::new().fg(t.accent).add_modifier(Modifier::BOLD),
            ),
            Span::styled(sanitize(&input.value()), Style::new().fg(t.fg)),
            Span::styled("   enter keep · esc clear", Style::new().fg(t.muted)),
        ]);
        f.render_widget(Paragraph::new(line), area);
        let x = area.x + prefix.len() as u16 + Line::raw(input.before_cursor()).width() as u16;
        f.set_cursor_position((x.min(area.right().saturating_sub(1)), area.y));
        return;
    }

    if let Some(status) = &app.status {
        let color = match status.level {
            Level::Info => t.info,
            Level::Warn => t.warn,
            Level::Error => t.error,
        };
        f.render_widget(
            Paragraph::new(Line::styled(sanitize(&status.text), Style::new().fg(color))),
            area,
        );
        return;
    }

    if !matches!(app.mode, Mode::Normal) {
        return; // dialogs show their own hints
    }

    let tail = [("s", "settings"), ("?", "help"), ("q", "quit")];
    let mut hints: Vec<(&str, &str)> = Vec::new();
    let width = |h: &[(&str, &str)]| -> usize {
        h.iter()
            .map(|(k, l)| k.chars().count() + l.chars().count() + 3)
            .sum::<usize>()
    };
    let budget = area.width as usize;
    let reserved = width(&tail);
    for hint in normal_hints(app) {
        let mut trial = hints.clone();
        trial.push(hint);
        if width(&trial) + reserved <= budget {
            hints = trial;
        } else {
            break;
        }
    }
    hints.extend(tail);
    f.render_widget(Paragraph::new(hint_line(app, &hints)), area);
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    )
}

fn dialog_block(
    app: &App,
    title: &str,
    border_color: ratatui::style::Color,
    g: &Glyphs,
) -> Block<'static> {
    let t = &app.theme;
    Block::bordered()
        .border_set(g.border)
        .border_style(Style::new().fg(border_color))
        .title(Line::styled(
            title.to_string(),
            Style::new().fg(border_color).add_modifier(Modifier::BOLD),
        ))
        .style(Style::new().bg(t.bg).fg(t.fg))
}

fn draw_confirm(f: &mut Frame, app: &App, confirm: &Confirm, area: Rect) {
    let t = &app.theme;
    let g = glyphs(app.unicode);
    let (title, color, headline, names, note, yes): (&str, _, String, Vec<String>, &str, &str) =
        match confirm {
            Confirm::Delete(paths) => (
                " Delete ",
                t.error,
                format!(
                    "Permanently delete {}?",
                    if paths.len() == 1 {
                        "this item".to_string()
                    } else {
                        format!("these {} items", paths.len())
                    }
                ),
                paths.iter().map(|p| display_name(p)).collect(),
                "Folders are removed with everything inside. There is no undo.",
                "delete",
            ),
            Confirm::Overwrite { op, plan } => {
                let clashes: Vec<String> = plan
                    .items
                    .iter()
                    .filter(|i| i.overwrite)
                    .map(|i| display_name(&i.dst))
                    .collect();
                (
                    " Overwrite ",
                    t.warn,
                    format!(
                        "{} already {} here. Replace {} to {}?",
                        if clashes.len() == 1 {
                            "1 item".to_string()
                        } else {
                            format!("{} items", clashes.len())
                        },
                        if clashes.len() == 1 {
                            "exists"
                        } else {
                            "exist"
                        },
                        if clashes.len() == 1 { "it" } else { "them" },
                        op.label()
                    ),
                    clashes,
                    "The existing items are deleted first. There is no undo.",
                    "replace",
                )
            }
        };

    let mut lines = vec![Line::styled(
        headline,
        Style::new().fg(t.fg).add_modifier(Modifier::BOLD),
    )];
    lines.push(Line::raw(""));
    for name in names.iter().take(6) {
        lines.push(Line::styled(format!("  {name}"), Style::new().fg(t.fg)));
    }
    if names.len() > 6 {
        lines.push(Line::styled(
            format!("  … and {} more", names.len() - 6),
            Style::new().fg(t.muted),
        ));
    }
    lines.push(Line::raw(""));
    lines.push(Line::styled(note, Style::new().fg(t.muted)));
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::styled("y", Style::new().fg(color).add_modifier(Modifier::BOLD)),
        Span::styled(format!(" {yes}"), Style::new().fg(t.fg)),
        Span::raw("    "),
        Span::styled(
            "n / esc",
            Style::new().fg(t.accent).add_modifier(Modifier::BOLD),
        ),
        Span::styled(" cancel", Style::new().fg(t.fg)),
    ]));

    let rect = centered(area, 64, lines.len() as u16 + 2);
    let block = dialog_block(app, title, color, &g);
    f.render_widget(Clear, rect);
    let inner = block.inner(rect);
    f.render_widget(block, rect);
    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
}

fn draw_help(f: &mut Frame, app: &App, g: &Glyphs, area: Rect) {
    let t = &app.theme;
    let rows: [(&str, &str); 20] = [
        ("↑ ↓  j k", "move selection"),
        ("enter", "open folder or file"),
        ("bksp  ←  h", "parent folder"),
        ("→  l", "enter folder"),
        ("space", "select / unselect, then move down"),
        ("a", "select all / none"),
        (
            "c  m",
            "copy / move: pick items, then go to the destination",
        ),
        ("p", "paste into the current folder"),
        ("r", "rename the item under the cursor"),
        ("d", "delete (always asks first)"),
        ("/", "filter by name"),
        ("esc", "clear filter, cancel copy/move, clear selection"),
        (".", "show / hide hidden files"),
        ("g  G", "jump to top / bottom"),
        ("pgup pgdn", "scroll a page"),
        ("?", "this help"),
        ("s", "settings and FOLD logo"),
        ("b", "bookmark this folder"),
        ("B", "browse saved bookmarks"),
        ("q  ctrl+c", "quit"),
    ];
    let mut lines: Vec<Line> = rows
        .iter()
        .map(|(k, d)| {
            Line::from(vec![
                Span::styled(
                    format!("{k:<12}"),
                    Style::new().fg(t.accent).add_modifier(Modifier::BOLD),
                ),
                Span::styled(d.to_string(), Style::new().fg(t.fg)),
            ])
        })
        .collect();
    lines.push(Line::raw(""));
    lines.push(Line::styled(
        format!("colours: {}", sanitize(&t.source)),
        Style::new().fg(t.muted),
    ));
    lines.push(Line::styled(
        "press any key to close",
        Style::new().fg(t.muted),
    ));

    let rect = centered(area, 72, lines.len() as u16 + 2);
    let block = dialog_block(app, " FOLD · keys ", t.accent, g);
    f.render_widget(Clear, rect);
    let inner = block.inner(rect);
    f.render_widget(block, rect);
    f.render_widget(Paragraph::new(lines), inner);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Theme;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use std::fs;

    fn render(app: &mut App, w: u16, h: u16) -> String {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| draw(f, app)).unwrap();
        let buf = term.backend().buffer().clone();
        let mut out = String::new();
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                out.push_str(buf[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    fn key(app: &mut App, c: KeyCode) {
        app.on_key(KeyEvent::new(c, KeyModifiers::NONE));
    }

    fn sample_app(unicode: bool) -> (tempfile::TempDir, App) {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir(d.path().join("projects")).unwrap();
        fs::write(d.path().join("notes.txt"), "hello").unwrap();
        let app = App::new(d.path(), Theme::builtin(true), unicode).unwrap();
        (d, app)
    }

    #[test]
    fn renders_header_list_and_footer() {
        let (_d, mut app) = sample_app(true);
        let out = render(&mut app, 100, 20);
        assert!(out.contains("┌─ FOLD"));
        assert!(out.contains("projects/"));
        assert!(out.contains("notes.txt"));
        assert!(out.contains("5 B"));
        assert!(out.contains("q quit"));
        assert!(out.contains("1/2"));
    }

    #[test]
    fn ascii_fallback_has_no_box_drawing() {
        let (_d, mut app) = sample_app(false);
        let out = render(&mut app, 100, 20);
        assert!(out.contains("+- FOLD"));
        assert!(out.contains("notes.txt"));
        assert!(
            out.chars().all(|c| c.is_ascii()
                || c == '\n'
                || c == '·'
                || c == '↑'
                || c == '↓'
                || c == '…'),
            "unexpected non-ascii in plain mode:\n{out}"
        );
    }

    #[test]
    fn footer_shrinks_on_narrow_terminals_but_keeps_quit() {
        let (_d, mut app) = sample_app(true);
        let out = render(&mut app, 40, 12);
        assert!(out.contains("q quit"));
        assert!(out.contains("? help"));
    }

    #[test]
    fn tiny_terminals_do_not_panic() {
        let (_d, mut app) = sample_app(true);
        for (w, h) in [(1, 1), (10, 3), (20, 5), (5, 40)] {
            render(&mut app, w, h);
            key(&mut app, KeyCode::Char('d'));
            render(&mut app, w, h);
            key(&mut app, KeyCode::Char('n'));
            key(&mut app, KeyCode::Char('r'));
            render(&mut app, w, h);
            key(&mut app, KeyCode::Esc);
            key(&mut app, KeyCode::Char('?'));
            render(&mut app, w, h);
            key(&mut app, KeyCode::Esc);
            key(&mut app, KeyCode::Char('/'));
            render(&mut app, w, h);
            key(&mut app, KeyCode::Esc);
        }
    }

    #[test]
    fn delete_dialog_names_the_target() {
        let (_d, mut app) = sample_app(true);
        key(&mut app, KeyCode::Char('d'));
        let out = render(&mut app, 100, 24);
        assert!(out.contains("Permanently delete"));
        assert!(out.contains("projects"));
        assert!(out.contains("n / esc cancel"));
    }

    #[test]
    fn long_lists_scroll_with_the_cursor() {
        let d = tempfile::tempdir().unwrap();
        for i in 0..100 {
            fs::write(d.path().join(format!("file{i:03}.txt")), "").unwrap();
        }
        let mut app = App::new(d.path(), Theme::builtin(true), true).unwrap();
        render(&mut app, 80, 12);
        for _ in 0..50 {
            key(&mut app, KeyCode::Down);
        }
        let out = render(&mut app, 80, 12);
        assert!(out.contains("file050.txt"));
        assert!(!out.contains("file000.txt"));
        assert!(out.contains("51/100"));
    }

    #[test]
    fn empty_folder_message() {
        let d = tempfile::tempdir().unwrap();
        let mut app = App::new(d.path(), Theme::builtin(true), true).unwrap();
        assert!(render(&mut app, 60, 10).contains("This folder is empty"));
    }
}
