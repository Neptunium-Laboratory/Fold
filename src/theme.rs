//! Theme loading.
//!
//! FOLD can borrow its colours from a Spicetify theme, but Spicetify is never
//! required. Nothing here touches the network; we only read local files.
//!
//! Lookup order (first one that works wins):
//!   1. `--theme <path>` on the command line
//!   2. the `FOLD_THEME` environment variable
//!   3. the theme Spicetify currently has active (`config-xpui.ini`)
//!   4. FOLD's built-in palette
//!
//! A "colour file" is an INI file like Spicetify's `color.ini`:
//!
//! ```ini
//! [Base]
//! main = 121212
//! text = ffffff
//! button = 1ed760
//! ```
//!
//! Hex values may be `rrggbb` or `rgb`, with or without a leading `#`.

use ratatui::style::Color;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

pub type Palette = HashMap<String, Rgb>;

/// Colour names we understand (the common Spicetify names).
pub const KNOWN_KEYS: [&str; 12] = [
    "main",
    "text",
    "subtext",
    "sidebar",
    "player",
    "card",
    "shadow",
    "selected-row",
    "button",
    "button-active",
    "notification",
    "notification-error",
];

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

/// Parse `rrggbb` / `rgb`, optionally prefixed with `#` or wrapped in quotes.
/// Anything after the first whitespace (e.g. a trailing comment) is ignored.
pub fn parse_hex(raw: &str) -> Option<Rgb> {
    let token = raw.split_whitespace().next()?;
    let s = token
        .trim_matches(|c| c == '"' || c == '\'')
        .trim_start_matches('#');
    let digits: Vec<u8> = s
        .chars()
        .map(|c| c.to_digit(16).map(|d| d as u8))
        .collect::<Option<Vec<_>>>()?;
    match digits.len() {
        3 => Some(Rgb(digits[0] * 17, digits[1] * 17, digits[2] * 17)),
        6 => Some(Rgb(
            digits[0] * 16 + digits[1],
            digits[2] * 16 + digits[3],
            digits[4] * 16 + digits[5],
        )),
        _ => None,
    }
}

#[derive(Debug, Default)]
pub struct Section {
    pub name: String,
    pub entries: Vec<(String, String)>,
}

/// Tiny INI parser: `[section]`, `key = value`, and `;` / `#` comment lines.
/// Keys before any `[section]` land in an unnamed first section.
pub fn parse_ini(text: &str) -> Vec<Section> {
    let mut out = vec![Section::default()];
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix('[') {
            if let Some(name) = rest.split(']').next() {
                out.push(Section {
                    name: name.trim().to_string(),
                    entries: Vec::new(),
                });
                continue;
            }
        }
        if let Some((k, v)) = line.split_once('=') {
            if let Some(section) = out.last_mut() {
                section
                    .entries
                    .push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
            }
        }
    }
    out
}

fn palette_of(section: &Section) -> Palette {
    section
        .entries
        .iter()
        .filter(|(k, _)| KNOWN_KEYS.contains(&k.as_str()))
        .filter_map(|(k, v)| parse_hex(v).map(|c| (k.clone(), c)))
        .collect()
}

/// Pick a colour scheme out of an INI file. If `scheme` names a section we use
/// it; otherwise we use the first section that has any recognised colours
/// (which is also what Spicetify does).
pub fn palette_from_ini(text: &str, scheme: Option<&str>) -> Result<Palette, String> {
    let sections = parse_ini(text);
    let mut candidates: Vec<(&Section, Palette)> = sections
        .iter()
        .map(|s| (s, palette_of(s)))
        .filter(|(_, p)| !p.is_empty())
        .collect();
    if candidates.is_empty() {
        return Err("no recognised colour entries (expected e.g. `main = 121212`)".into());
    }
    if let Some(want) = scheme {
        if let Some(pos) = candidates
            .iter()
            .position(|(s, _)| s.name.eq_ignore_ascii_case(want))
        {
            return Ok(candidates.swap_remove(pos).1);
        }
    }
    Ok(candidates.remove(0).1)
}

// ---------------------------------------------------------------------------
// Colour maths
// ---------------------------------------------------------------------------

fn luminance(c: Rgb) -> f64 {
    fn channel(v: u8) -> f64 {
        let s = v as f64 / 255.0;
        if s <= 0.03928 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * channel(c.0) + 0.7152 * channel(c.1) + 0.0722 * channel(c.2)
}

/// WCAG contrast ratio, 1.0 (identical) to 21.0 (black on white).
pub fn contrast(a: Rgb, b: Rgb) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

fn mix(a: Rgb, b: Rgb, t: f64) -> Rgb {
    let lerp = |x: u8, y: u8| (x as f64 + (y as f64 - x as f64) * t).round() as u8;
    Rgb(lerp(a.0, b.0), lerp(a.1, b.1), lerp(a.2, b.2))
}

fn best_text_on(bg: Rgb) -> Rgb {
    let white = Rgb(255, 255, 255);
    let black = Rgb(0, 0, 0);
    if contrast(white, bg) >= contrast(black, bg) {
        white
    } else {
        black
    }
}

/// Use `c` if it is readable on `bg`, otherwise `fallback`.
fn readable(c: Option<Rgb>, bg: Rgb, min_ratio: f64, fallback: Rgb) -> Rgb {
    match c {
        Some(c) if contrast(c, bg) >= min_ratio => c,
        _ => fallback,
    }
}

/// Map a 24-bit colour to the nearest xterm-256 palette index.
pub fn to_indexed(c: Rgb) -> u8 {
    const LEVELS: [i32; 6] = [0, 95, 135, 175, 215, 255];
    let (r, g, b) = (c.0 as i32, c.1 as i32, c.2 as i32);
    let nearest = |v: i32| {
        (0..6usize)
            .min_by_key(|&i| (LEVELS[i] - v).abs())
            .unwrap_or(0)
    };
    let (ri, gi, bi) = (nearest(r), nearest(g), nearest(b));
    let cube_rgb = (LEVELS[ri], LEVELS[gi], LEVELS[bi]);
    let cube_idx = 16 + 36 * ri + 6 * gi + bi;

    let avg = (r + g + b) / 3;
    let gray_step = (((avg - 8).max(0)) / 10).min(23);
    let gv = 8 + 10 * gray_step;

    let dist = |p: (i32, i32, i32)| (p.0 - r).pow(2) + (p.1 - g).pow(2) + (p.2 - b).pow(2);
    if dist((gv, gv, gv)) < dist(cube_rgb) {
        (232 + gray_step) as u8
    } else {
        cube_idx as u8
    }
}

fn to_color(c: Rgb, truecolor: bool) -> Color {
    if truecolor {
        Color::Rgb(c.0, c.1, c.2)
    } else {
        Color::Indexed(to_indexed(c))
    }
}

/// Do we think the terminal can show 24-bit colour?
/// `FOLD_COLORS=truecolor|256` overrides detection.
pub fn detect_truecolor() -> bool {
    match std::env::var("FOLD_COLORS").ok().as_deref() {
        Some("truecolor") => return true,
        Some("256") => return false,
        _ => {}
    }
    std::env::var("COLORTERM")
        .map(|v| {
            let v = v.to_ascii_lowercase();
            v.contains("truecolor") || v.contains("24bit")
        })
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Theme
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct Theme {
    pub bg: Color,
    pub fg: Color,
    pub muted: Color,
    pub border: Color,
    pub accent: Color,
    pub cursor_bg: Color,
    pub cursor_fg: Color,
    pub info: Color,
    pub warn: Color,
    pub error: Color,
    /// Human-readable description of where the colours came from.
    pub source: String,
}

/// The restrained built-in palette (dark, green accent).
pub fn builtin_palette() -> Palette {
    let mut p = Palette::new();
    for (k, hex) in [
        ("main", "121212"),
        ("text", "e6e6e6"),
        ("subtext", "a0a0a0"),
        ("card", "1f1f1f"),
        ("button", "1ed760"),
        ("notification", "5aa9ff"),
        ("notification-error", "ff6b6b"),
    ] {
        if let Some(c) = parse_hex(hex) {
            p.insert(k.to_string(), c);
        }
    }
    p
}

impl Theme {
    /// Build a theme from whatever colours the palette has. Missing or
    /// unreadable colours are replaced with safe ones, so any palette
    /// (even a partial or light one) gives a usable UI.
    pub fn from_palette(p: &Palette, truecolor: bool, source: String) -> Theme {
        let get = |k: &str| p.get(k).copied();

        let bg = get("main").unwrap_or(Rgb(0x12, 0x12, 0x12));
        let dark = contrast(Rgb(255, 255, 255), bg) >= contrast(Rgb(0, 0, 0), bg);

        let fg = readable(get("text"), bg, 4.5, best_text_on(bg));
        let muted = readable(get("subtext"), bg, 3.0, mix(bg, fg, 0.6));
        let border = mix(bg, fg, 0.30);

        // Subtle surface used behind the cursor row.
        let surface = [get("card"), get("sidebar"), get("player")]
            .into_iter()
            .flatten()
            .find(|c| contrast(*c, bg) >= 1.05)
            .unwrap_or_else(|| mix(bg, fg, 0.10));
        let cursor_fg = readable(
            get("selected-row"),
            surface,
            4.5,
            readable(Some(fg), surface, 4.5, best_text_on(surface)),
        );

        // Accent: the theme's button colour (used for the selected/focused bits).
        let accent = [get("button"), get("button-active"), get("selected-row")]
            .into_iter()
            .flatten()
            .find(|c| contrast(*c, bg) >= 3.0)
            .unwrap_or(fg);

        let info = readable(get("notification"), bg, 3.0, accent);
        let (warn_default, error_default) = if dark {
            (Rgb(0xe6, 0xc8, 0x6e), Rgb(0xff, 0x6b, 0x6b))
        } else {
            (Rgb(0x8a, 0x61, 0x00), Rgb(0xb3, 0x26, 0x1e))
        };
        let warn = warn_default;
        let error = readable(get("notification-error"), bg, 4.5, error_default);

        let c = |rgb| to_color(rgb, truecolor);
        Theme {
            bg: c(bg),
            fg: c(fg),
            muted: c(muted),
            border: c(border),
            accent: c(accent),
            cursor_bg: c(surface),
            cursor_fg: c(cursor_fg),
            info: c(info),
            warn: c(warn),
            error: c(error),
            source,
        }
    }

    pub fn builtin(truecolor: bool) -> Theme {
        Theme::from_palette(&builtin_palette(), truecolor, "built-in palette".into())
    }

    /// Use the active terminal's default foreground/background and ANSI
    /// palette. Terminals already themed by Matugen then pass those colors
    /// through to FOLD without needing to regenerate them from an image.
    pub fn terminal() -> Theme {
        Theme {
            bg: Color::Reset,
            fg: Color::Reset,
            muted: Color::DarkGray,
            border: Color::DarkGray,
            accent: Color::Green,
            cursor_bg: Color::DarkGray,
            cursor_fg: Color::Reset,
            info: Color::Cyan,
            warn: Color::Yellow,
            error: Color::Red,
            source: "System / terminal colors".into(),
        }
    }
}

/// Built-in presets shown in the Settings page. Matugen is generated from an
/// image separately, after the user supplies its path.
pub fn preset(index: usize) -> Theme {
    let (name, colors): (&str, &[(&str, &str)]) = match index {
        0 => (
            "Catppuccin Mocha",
            &[
                ("main", "1e1e2e"),
                ("text", "cdd6f4"),
                ("subtext", "a6adc8"),
                ("card", "313244"),
                ("button", "cba6f7"),
            ],
        ),
        1 => (
            "Osaka Jane",
            &[
                ("main", "0b1414"),
                ("text", "d8e5dc"),
                ("subtext", "87a89a"),
                ("card", "162725"),
                ("button", "9ece6a"),
            ],
        ),
        2 => (
            "One Dark",
            &[
                ("main", "282c34"),
                ("text", "abb2bf"),
                ("subtext", "7f848e"),
                ("card", "3e4451"),
                ("button", "61afef"),
            ],
        ),
        3 => (
            "Gruvbox",
            &[
                ("main", "282828"),
                ("text", "ebdbb2"),
                ("subtext", "a89984"),
                ("card", "3c3836"),
                ("button", "fabd2f"),
            ],
        ),
        _ => return Theme::builtin(detect_truecolor()),
    };
    let palette = colors
        .iter()
        .filter_map(|(key, value)| parse_hex(value).map(|rgb| ((*key).to_string(), rgb)))
        .collect();
    Theme::from_palette(&palette, detect_truecolor(), name.into())
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

pub struct Loaded {
    pub theme: Theme,
    /// Problems worth telling the user about (e.g. a bad `--theme` path).
    pub warnings: Vec<String>,
}

/// Where Spicetify keeps its configuration on Linux.
pub fn spicetify_dir() -> Option<PathBuf> {
    let nonempty = |k: &str| {
        std::env::var_os(k)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    if let Some(p) = nonempty("SPICETIFY_CONFIG") {
        return Some(p);
    }
    if let Some(p) = nonempty("XDG_CONFIG_HOME") {
        return Some(p.join("spicetify"));
    }
    nonempty("HOME").map(|h| h.join(".config").join("spicetify"))
}

/// Read a colour file. A directory is accepted too (we look for `color.ini`).
pub fn load_file(path: &Path, scheme: Option<&str>) -> Result<Palette, String> {
    let file = if path.is_dir() {
        path.join("color.ini")
    } else {
        path.to_path_buf()
    };
    let text = fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
    palette_from_ini(&text, scheme).map_err(|e| format!("{}: {e}", file.display()))
}

/// Find the active Spicetify theme. `Ok(None)` means "Spicetify isn't set up",
/// which is perfectly normal and not worth a warning.
pub fn from_spicetify(dir: &Path) -> Result<Option<(Palette, String)>, String> {
    let cfg = dir.join("config-xpui.ini");
    let text = match fs::read_to_string(&cfg) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("{}: {e}", cfg.display())),
    };
    let sections = parse_ini(&text);
    let setting = |key: &str| {
        sections
            .iter()
            .filter(|s| s.name.eq_ignore_ascii_case("Setting"))
            .flat_map(|s| s.entries.iter())
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
            .filter(|v| !v.is_empty())
    };
    let Some(theme) = setting("current_theme") else {
        return Ok(None);
    };
    let scheme = setting("color_scheme");
    let file = dir.join("Themes").join(&theme).join("color.ini");
    let palette = load_file(&file, scheme.as_deref())?;
    let label = match &scheme {
        Some(s) => format!("Spicetify theme '{theme}' (scheme '{s}')"),
        None => format!("Spicetify theme '{theme}'"),
    };
    Ok(Some((palette, label)))
}

pub fn load(cli_theme: Option<PathBuf>) -> Loaded {
    let env_theme = std::env::var_os("FOLD_THEME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from);
    load_from(cli_theme, env_theme, spicetify_dir(), detect_truecolor())
}

/// Ask Matugen to derive a dark Material You palette from an image. This is
/// opt-in and keeps Matugen an optional runtime dependency.
pub fn load_matugen(image: &Path) -> Loaded {
    let executable = std::env::var("MATUGEN").unwrap_or_else(|_| "matugen".into());
    let result = std::process::Command::new(&executable)
        .arg("image")
        .arg(image)
        .arg("--json")
        .arg("hex")
        .arg("-m")
        .arg("dark")
        .output();
    let parsed = result
        .map_err(|e| format!("cannot run {executable}: {e}"))
        .and_then(|out| {
            if !out.status.success() {
                return Err(format!(
                    "Matugen failed: {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                ));
            }
            let value: serde_json::Value = serde_json::from_slice(&out.stdout)
                .map_err(|e| format!("Matugen returned invalid JSON: {e}"))?;
            let colors = value
                .get("colors")
                .and_then(|v| v.get("dark"))
                .ok_or_else(|| "Matugen JSON has no dark color scheme".to_string())?;
            let mut p = Palette::new();
            let mappings = [
                ("background", "main"),
                ("surface", "card"),
                ("on_surface", "text"),
                ("on_background", "text"),
                ("primary", "button"),
                ("secondary", "button-active"),
                ("outline", "subtext"),
                ("error", "notification-error"),
                ("shadow", "shadow"),
            ];
            for (from, to) in mappings {
                if let Some(raw) = colors.get(from).and_then(|v| v.as_str()) {
                    if let Some(rgb) = parse_hex(raw) {
                        p.insert(to.into(), rgb);
                    }
                }
            }
            if !p.contains_key("main") || !p.contains_key("text") || !p.contains_key("button") {
                return Err("Matugen palette is missing required colors".into());
            }
            Ok(p)
        });
    match parsed {
        Ok(p) => Loaded {
            theme: Theme::from_palette(
                &p,
                detect_truecolor(),
                format!("Matugen: {}", image.display()),
            ),
            warnings: Vec::new(),
        },
        Err(e) => Loaded {
            theme: Theme::builtin(detect_truecolor()),
            warnings: vec![e],
        },
    }
}

pub fn load_from(
    cli: Option<PathBuf>,
    env: Option<PathBuf>,
    spicetify: Option<PathBuf>,
    truecolor: bool,
) -> Loaded {
    let mut warnings = Vec::new();

    for (label, path) in [("--theme", cli), ("FOLD_THEME", env)] {
        let Some(path) = path else { continue };
        match load_file(&path, None) {
            Ok(p) => {
                let source = format!("{label}: {}", path.display());
                return Loaded {
                    theme: Theme::from_palette(&p, truecolor, source),
                    warnings,
                };
            }
            Err(e) => warnings.push(format!("{label} ignored: {e}")),
        }
    }

    if let Some(dir) = spicetify {
        match from_spicetify(&dir) {
            Ok(Some((p, label))) => {
                return Loaded {
                    theme: Theme::from_palette(&p, truecolor, label),
                    warnings,
                };
            }
            Ok(None) => {}
            Err(e) => warnings.push(format!("Spicetify theme ignored: {e}")),
        }
    }

    Loaded {
        theme: Theme::builtin(truecolor),
        warnings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_parsing() {
        assert_eq!(parse_hex("1ed760"), Some(Rgb(0x1e, 0xd7, 0x60)));
        assert_eq!(parse_hex("#FFF"), Some(Rgb(255, 255, 255)));
        assert_eq!(parse_hex("\"000000\""), Some(Rgb(0, 0, 0)));
        assert_eq!(parse_hex("ffffff ; white"), Some(Rgb(255, 255, 255)));
        assert_eq!(parse_hex("12345"), None);
        assert_eq!(parse_hex("gggggg"), None);
        assert_eq!(parse_hex(""), None);
    }

    #[test]
    fn ini_sections_comments_and_scheme() {
        let text = "; comment\n[Base]\nmain = 111111\r\ntext = #ffffff\nbogus = zzz\n\n[Light]\nmain = eeeeee\ntext = 000000\n";
        let base = palette_from_ini(text, None).unwrap();
        assert_eq!(base["main"], Rgb(0x11, 0x11, 0x11));
        assert!(!base.contains_key("bogus"));
        let light = palette_from_ini(text, Some("light")).unwrap();
        assert_eq!(light["main"], Rgb(0xee, 0xee, 0xee));
        // Unknown scheme falls back to the first section.
        let other = palette_from_ini(text, Some("nope")).unwrap();
        assert_eq!(other["main"], Rgb(0x11, 0x11, 0x11));
    }

    #[test]
    fn flat_file_without_sections_works() {
        let p = palette_from_ini("main=000000\ntext=ffffff\n", None).unwrap();
        assert_eq!(p.len(), 2);
    }

    #[test]
    fn empty_or_garbage_is_an_error() {
        assert!(palette_from_ini("", None).is_err());
        assert!(palette_from_ini("hello world", None).is_err());
    }

    #[test]
    fn unreadable_colours_are_replaced() {
        // text barely differs from the background: must not be used.
        let p = palette_from_ini(
            "main = 101010\ntext = 111111\nnotification-error = 121212\n",
            None,
        )
        .unwrap();
        let t = Theme::from_palette(&p, true, "test".into());
        assert_eq!(t.fg, Color::Rgb(255, 255, 255));
        assert_eq!(t.error, Color::Rgb(0xff, 0x6b, 0x6b));
    }

    #[test]
    fn light_backgrounds_get_dark_text_and_dark_warnings() {
        let p = palette_from_ini("main = ffffff\n", None).unwrap();
        let t = Theme::from_palette(&p, true, "test".into());
        assert_eq!(t.fg, Color::Rgb(0, 0, 0));
        assert_eq!(t.error, Color::Rgb(0xb3, 0x26, 0x1e));
    }

    #[test]
    fn contrast_extremes() {
        assert!((contrast(Rgb(0, 0, 0), Rgb(255, 255, 255)) - 21.0).abs() < 0.01);
        assert!((contrast(Rgb(5, 5, 5), Rgb(5, 5, 5)) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn indexed_colours() {
        assert_eq!(to_indexed(Rgb(0, 0, 0)), 16);
        assert_eq!(to_indexed(Rgb(255, 255, 255)), 231);
        // Dark gray should use the grayscale ramp.
        assert!((232..=255).contains(&to_indexed(Rgb(0x12, 0x12, 0x12))));
    }

    #[test]
    fn spicetify_discovery_and_priority() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("Themes/text")).unwrap();
        fs::write(
            root.join("config-xpui.ini"),
            "[Setting]\ncurrent_theme            = text\ncolor_scheme = Dark\n",
        )
        .unwrap();
        fs::write(
            root.join("Themes/text/color.ini"),
            "[Base]\nmain = ffffff\n[Dark]\nmain = 000000\nbutton = 1ed760\n",
        )
        .unwrap();

        let loaded = load_from(None, None, Some(root.to_path_buf()), true);
        assert!(loaded.warnings.is_empty());
        assert!(loaded.theme.source.contains("'text'"));
        assert_eq!(loaded.theme.bg, Color::Rgb(0, 0, 0));
        assert_eq!(loaded.theme.accent, Color::Rgb(0x1e, 0xd7, 0x60));

        // An explicit file beats Spicetify.
        let custom = root.join("mine.ini");
        fs::write(&custom, "main = 222222\n").unwrap();
        let loaded = load_from(None, Some(custom), Some(root.to_path_buf()), true);
        assert_eq!(loaded.theme.bg, Color::Rgb(0x22, 0x22, 0x22));
    }

    #[test]
    fn bad_explicit_path_warns_and_falls_back() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = load_from(
            Some(dir.path().join("missing.ini")),
            None,
            Some(dir.path().join("no-spicetify")),
            true,
        );
        assert_eq!(loaded.warnings.len(), 1);
        assert_eq!(loaded.theme.source, "built-in palette");
    }

    #[test]
    fn missing_spicetify_is_silent() {
        let dir = tempfile::tempdir().unwrap();
        let loaded = load_from(None, None, Some(dir.path().to_path_buf()), true);
        assert!(loaded.warnings.is_empty());
        assert_eq!(loaded.theme.source, "built-in palette");
    }
}
