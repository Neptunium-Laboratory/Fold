//! Command-line arguments. Hand-rolled on purpose: FOLD only has a few
//! options, so a parsing library would be more code than it saves.

use std::ffi::OsString;
use std::path::PathBuf;

pub const HELP: &str = "\
FOLD - your files, in flow. A keyboard-first terminal file manager.

USAGE:
    fold [OPTIONS] [DIRECTORY]

OPTIONS:
    --theme <PATH>   Colour file (Spicetify-style color.ini) or a folder containing one
    --matugen <IMAGE> Generate FOLD colours from an image using Matugen
    -h, --help       Show this help
    -V, --version    Show the version

ENVIRONMENT:
    FOLD_THEME       Same as --theme (the flag wins if both are set)
    MATUGEN          Optional executable used by --matugen (default: matugen)
    FOLD_OPENER      Program used to open files (default: xdg-open)
    FOLD_COLORS      truecolor | 256   (override colour-depth detection)
    FOLD_ASCII       1 = plain ASCII instead of box-drawing characters

Press ? inside FOLD for the list of keys.
";

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Cli {
    pub start: Option<PathBuf>,
    pub theme: Option<PathBuf>,
    pub matugen_image: Option<PathBuf>,
    pub help: bool,
    pub version: bool,
}

pub fn parse<I: IntoIterator<Item = OsString>>(args: I) -> Result<Cli, String> {
    let mut cli = Cli::default();
    let mut args = args.into_iter();
    let mut only_paths = false;
    while let Some(arg) = args.next() {
        let text = arg.to_string_lossy().into_owned();
        if only_paths || !text.starts_with('-') || text == "-" {
            if cli.start.is_some() {
                return Err("only one directory can be given".into());
            }
            cli.start = Some(PathBuf::from(arg));
            continue;
        }
        match text.as_str() {
            "--" => only_paths = true,
            "-h" | "--help" => cli.help = true,
            "-V" | "--version" => cli.version = true,
            "--theme" => {
                let value = args.next().ok_or("--theme needs a path")?;
                if value.is_empty() || value.to_string_lossy().starts_with('-') {
                    return Err(
                        "--theme needs a path (use --theme=PATH for paths beginning with '-')"
                            .into(),
                    );
                }
                cli.theme = Some(PathBuf::from(value));
            }
            "--matugen" => {
                let value = args.next().ok_or("--matugen needs an image path")?;
                if value.is_empty() || value.to_string_lossy().starts_with('-') {
                    return Err("--matugen needs an image path".into());
                }
                cli.matugen_image = Some(PathBuf::from(value));
            }
            other => {
                if let Some(value) = other.strip_prefix("--theme=") {
                    if value.is_empty() {
                        return Err("--theme needs a path".into());
                    }
                    cli.theme = Some(PathBuf::from(value));
                } else if let Some(value) = other.strip_prefix("--matugen=") {
                    if value.is_empty() {
                        return Err("--matugen needs an image path".into());
                    }
                    cli.matugen_image = Some(PathBuf::from(value));
                } else {
                    return Err(format!("unknown option '{other}' (try --help)"));
                }
            }
        }
    }
    Ok(cli)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<Cli, String> {
        parse(args.iter().map(OsString::from))
    }

    #[test]
    fn parses_flags_and_paths() {
        assert_eq!(p(&[]).unwrap(), Cli::default());
        assert_eq!(p(&["/tmp"]).unwrap().start, Some(PathBuf::from("/tmp")));
        assert_eq!(
            p(&["--theme", "x.ini", "/tmp"]).unwrap().theme,
            Some(PathBuf::from("x.ini"))
        );
        assert_eq!(
            p(&["--theme=y.ini"]).unwrap().theme,
            Some(PathBuf::from("y.ini"))
        );
        assert_eq!(
            p(&["--theme=-dark.ini"]).unwrap().theme,
            Some(PathBuf::from("-dark.ini"))
        );
        assert!(p(&["-h"]).unwrap().help);
        assert!(p(&["--version"]).unwrap().version);
        assert_eq!(
            p(&["--", "-weird-dir"]).unwrap().start,
            Some(PathBuf::from("-weird-dir"))
        );
    }

    #[test]
    fn rejects_bad_input() {
        assert!(p(&["--nope"]).is_err());
        assert!(p(&["--theme"]).is_err());
        assert!(p(&["--theme", "--help"]).is_err());
        assert!(p(&["--theme="]).is_err());
        assert!(p(&["a", "b"]).is_err());
    }
}
