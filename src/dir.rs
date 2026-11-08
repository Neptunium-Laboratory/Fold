//! Directory state: reading a folder, sorting, hiding, filtering.
//!
//! Nothing in here draws anything or reacts to keys; it only answers
//! "what is in this folder?" so it is easy to test.

use std::borrow::Cow;
use std::cmp::Reverse;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Entry {
    /// The real file name (may not be valid UTF-8).
    pub name: OsString,
    /// Safe-to-print version of the name: invalid UTF-8 and control
    /// characters are replaced so they can't garble or hijack the terminal.
    pub display: String,
    /// True when `display` differs from the real name.
    pub lossy: bool,
    pub path: PathBuf,
    /// A directory, or a symlink that points at a directory.
    pub is_dir: bool,
    pub is_symlink: bool,
    pub broken_link: bool,
    pub size: u64,
    pub hidden: bool,
}

pub struct Listing {
    pub entries: Vec<Entry>,
    /// Entries the OS refused to describe while reading the folder.
    pub skipped: usize,
}

/// Replace control characters (ESC, newline, ...) with U+FFFD.
pub fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_control() { '\u{FFFD}' } else { c })
        .collect()
}

impl Entry {
    fn from_dir_entry(item: &fs::DirEntry) -> Entry {
        let name = item.file_name();
        let path = item.path();

        let cow = name.to_string_lossy();
        let invalid_utf8 = matches!(cow, Cow::Owned(_));
        let display = sanitize(&cow);
        let lossy = invalid_utf8 || display != *cow;

        let hidden = name.as_bytes().first() == Some(&b'.');

        let (is_symlink, is_dir, broken_link, size) = match item.metadata() {
            Ok(meta) if meta.file_type().is_symlink() => match fs::metadata(&path) {
                Ok(target) => (true, target.is_dir(), false, target.len()),
                Err(_) => (true, false, true, 0),
            },
            Ok(meta) => (false, meta.is_dir(), false, meta.len()),
            Err(_) => (false, false, false, 0),
        };

        Entry {
            name,
            display,
            lossy,
            path,
            is_dir,
            is_symlink,
            broken_link,
            size,
            hidden,
        }
    }
}

/// Read a folder. Fails only if the folder itself can't be opened.
pub fn list_dir(dir: &Path) -> io::Result<Listing> {
    let mut entries = Vec::new();
    let mut skipped = 0;
    for item in fs::read_dir(dir)? {
        match item {
            Ok(item) => entries.push(Entry::from_dir_entry(&item)),
            Err(_) => skipped += 1,
        }
    }
    // Folders first, then case-insensitive by name.
    entries.sort_by_cached_key(|e| (Reverse(e.is_dir), e.display.to_lowercase(), e.name.clone()));
    Ok(Listing { entries, skipped })
}

/// Indices (into `entries`) of the entries that should be on screen.
pub fn visible_indices(entries: &[Entry], show_hidden: bool, query: &str) -> Vec<usize> {
    let q = query.to_lowercase();
    entries
        .iter()
        .enumerate()
        .filter(|(_, e)| show_hidden || !e.hidden)
        .filter(|(_, e)| q.is_empty() || e.display.to_lowercase().contains(&q))
        .map(|(i, _)| i)
        .collect()
}

/// The folder above `path`, or `None` at the root.
pub fn parent_dir(path: &Path) -> Option<PathBuf> {
    path.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "K", "M", "G", "T"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;
    use std::os::unix::fs::symlink;

    fn names(l: &Listing) -> Vec<&str> {
        l.entries.iter().map(|e| e.display.as_str()).collect()
    }

    #[test]
    fn lists_folders_first_then_alphabetical() {
        let d = tempfile::tempdir().unwrap();
        fs::write(d.path().join("b.txt"), "b").unwrap();
        fs::write(d.path().join("A.txt"), "a").unwrap();
        fs::create_dir(d.path().join("zeta")).unwrap();
        fs::create_dir(d.path().join("alpha")).unwrap();
        let l = list_dir(d.path()).unwrap();
        assert_eq!(names(&l), ["alpha", "zeta", "A.txt", "b.txt"]);
    }

    #[test]
    fn hidden_toggle_and_filter() {
        let d = tempfile::tempdir().unwrap();
        for n in [".secret", "notes.md", "Notes.txt", "photo.png"] {
            fs::write(d.path().join(n), "").unwrap();
        }
        let l = list_dir(d.path()).unwrap();
        let shown = |hidden, q| -> Vec<&str> {
            visible_indices(&l.entries, hidden, q)
                .into_iter()
                .map(|i| l.entries[i].display.as_str())
                .collect()
        };
        assert_eq!(shown(false, ""), ["notes.md", "Notes.txt", "photo.png"]);
        assert_eq!(
            shown(true, ""),
            [".secret", "notes.md", "Notes.txt", "photo.png"]
        );
        assert_eq!(shown(false, "NOTES"), ["notes.md", "Notes.txt"]);
        assert_eq!(shown(false, ".sec"), Vec::<&str>::new());
        assert_eq!(shown(true, ".sec"), [".secret"]);
        assert_eq!(shown(false, "zzz"), Vec::<&str>::new());
    }

    #[test]
    fn invalid_utf8_and_control_characters_are_safe() {
        let d = tempfile::tempdir().unwrap();
        let bad = OsStr::from_bytes(b"bad\xff\xfename");
        fs::write(d.path().join(bad), "").unwrap();
        fs::write(d.path().join("esc\u{1b}[31mred"), "").unwrap();
        let l = list_dir(d.path()).unwrap();
        assert_eq!(l.entries.len(), 2);
        for e in &l.entries {
            assert!(e.lossy, "{:?}", e.name);
            assert!(!e.display.chars().any(|c| c.is_control()));
        }
    }

    #[test]
    fn symlinks_to_dirs_are_navigable_and_broken_ones_flagged() {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir(d.path().join("real")).unwrap();
        symlink(d.path().join("real"), d.path().join("link")).unwrap();
        symlink(d.path().join("gone"), d.path().join("dead")).unwrap();
        let l = list_dir(d.path()).unwrap();
        let get = |n: &str| l.entries.iter().find(|e| e.display == n).unwrap();
        assert!(get("link").is_dir && get("link").is_symlink);
        assert!(get("dead").broken_link && !get("dead").is_dir);
    }

    #[test]
    fn unreadable_folder_is_an_error_not_a_panic() {
        let d = tempfile::tempdir().unwrap();
        assert!(list_dir(&d.path().join("missing")).is_err());
    }

    #[test]
    fn parent_navigation() {
        assert_eq!(parent_dir(Path::new("/a/b")), Some(PathBuf::from("/a")));
        assert_eq!(parent_dir(Path::new("/a")), Some(PathBuf::from("/")));
        assert_eq!(parent_dir(Path::new("/")), None);
    }

    #[test]
    fn sizes_are_readable() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(1023), "1023 B");
        assert_eq!(human_size(1536), "1.5 K");
        assert_eq!(human_size(5 * 1024 * 1024), "5.0 M");
    }
}
