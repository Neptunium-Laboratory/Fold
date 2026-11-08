//! File operations. No terminal code in here.
//!
//! Safety rules this module enforces:
//!   * Symbolic links are never followed when deleting or copying: a link is
//!     removed / copied *as a link*.
//!   * You can't copy or move a folder into itself.
//!   * Anything that would overwrite an existing item is flagged in the plan
//!     (`PasteItem::overwrite`) so the UI can ask first.
//!   * Every failure is returned as a readable message, never swallowed.

use crate::dir::sanitize;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

/// "Cross-device link" error number on Linux: `rename` can't move between
/// filesystems, so we fall back to copy + delete.
const EXDEV: i32 = 18;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Copy,
    Move,
}

impl Op {
    pub fn label(self) -> &'static str {
        match self {
            Op::Copy => "copy",
            Op::Move => "move",
        }
    }
    pub fn past(self) -> &'static str {
        match self {
            Op::Copy => "Copied",
            Op::Move => "Moved",
        }
    }
}

#[derive(Clone, Debug)]
pub struct PasteItem {
    pub src: PathBuf,
    pub dst: PathBuf,
    /// `dst` already exists and will be replaced (the UI must confirm first).
    pub overwrite: bool,
}

#[derive(Clone, Debug, Default)]
pub struct PastePlan {
    pub items: Vec<PasteItem>,
    /// Things we refused to plan (with the reason), shown to the user.
    pub problems: Vec<String>,
}

#[derive(Debug, Default)]
pub struct Report {
    pub done: usize,
    pub errors: Vec<String>,
}

pub fn display_name(path: &Path) -> String {
    match path.file_name() {
        Some(n) => sanitize(&n.to_string_lossy()),
        None => sanitize(&path.to_string_lossy()),
    }
}

fn exists(path: &Path) -> bool {
    // symlink_metadata does not follow links, so a broken link still "exists".
    fs::symlink_metadata(path).is_ok()
}

/// Work out what pasting `sources` into `dest_dir` would do, without doing it.
pub fn plan_paste(sources: &[PathBuf], dest_dir: &Path, op: Op) -> PastePlan {
    let mut plan = PastePlan::default();
    let dest_canon = match fs::canonicalize(dest_dir) {
        Ok(p) => p,
        Err(e) => {
            plan.problems
                .push(format!("{}: {e}", display_name(dest_dir)));
            return plan;
        }
    };

    for src in sources {
        let label = display_name(src);
        let Some(name) = src.file_name() else {
            plan.problems.push(format!("{label}: has no file name"));
            continue;
        };
        // Canonicalise the *folder* only, so a symlink source stays a symlink.
        let src_abs = match src.parent().map(fs::canonicalize) {
            Some(Ok(p)) => p.join(name),
            _ => {
                plan.problems
                    .push(format!("{label}: its folder is not accessible"));
                continue;
            }
        };
        let Ok(meta) = fs::symlink_metadata(&src_abs) else {
            plan.problems.push(format!("{label}: no longer exists"));
            continue;
        };
        let is_real_dir = meta.is_dir();
        let mut dst = dest_canon.join(name);

        if dst == src_abs {
            match op {
                Op::Move => {
                    plan.problems
                        .push(format!("{label}: is already in this folder"));
                    continue;
                }
                // Copying into the same folder makes "name copy".
                Op::Copy => dst = unique_copy_name(&dest_canon, name),
            }
        } else {
            if is_real_dir && dst.starts_with(&src_abs) {
                plan.problems.push(format!(
                    "{label}: cannot {} a folder into itself",
                    op.label()
                ));
                continue;
            }
            if src_abs.starts_with(&dst) {
                plan.problems
                    .push(format!("{label}: the item it would replace contains it"));
                continue;
            }
        }

        let overwrite = exists(&dst);
        plan.items.push(PasteItem {
            src: src_abs,
            dst,
            overwrite,
        });
    }
    plan
}

/// `name copy`, `name copy 2`, ... whichever is free.
pub fn unique_copy_name(dir: &Path, name: &OsStr) -> PathBuf {
    let mut n = 1;
    loop {
        let mut candidate = name.to_os_string();
        if n == 1 {
            candidate.push(" copy");
        } else {
            candidate.push(format!(" copy {n}"));
        }
        let path = dir.join(candidate);
        if !exists(&path) {
            return path;
        }
        n += 1;
    }
}

/// Carry out a plan. Keeps going after a failure and reports every error.
pub fn execute(op: Op, plan: &PastePlan) -> Report {
    let mut report = Report::default();
    for item in &plan.items {
        let label = display_name(&item.src);
        if item.overwrite {
            if let Err(e) = delete_path(&item.dst) {
                report
                    .errors
                    .push(format!("{label}: could not replace the existing item: {e}"));
                continue;
            }
        }
        let result = match op {
            Op::Copy => copy_path(&item.src, &item.dst),
            Op::Move => move_path(&item.src, &item.dst),
        };
        match result {
            Ok(()) => report.done += 1,
            Err(e) => report.errors.push(format!("{label}: {e}")),
        }
    }
    report
}

fn ctx(path: &Path, e: io::Error) -> io::Error {
    io::Error::new(e.kind(), format!("{}: {e}", display_name(path)))
}

/// Copy a file, folder (recursively) or symlink. `dst` must not exist.
/// Permission bits are preserved; owner and timestamps are not.
pub fn copy_path(src: &Path, dst: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(src).map_err(|e| ctx(src, e))?;
    let kind = meta.file_type();
    if kind.is_symlink() {
        let target = fs::read_link(src).map_err(|e| ctx(src, e))?;
        symlink(target, dst).map_err(|e| ctx(dst, e))
    } else if kind.is_dir() {
        fs::create_dir(dst).map_err(|e| ctx(dst, e))?;
        for item in fs::read_dir(src).map_err(|e| ctx(src, e))? {
            let item = item.map_err(|e| ctx(src, e))?;
            copy_path(&item.path(), &dst.join(item.file_name()))?;
        }
        // Set permissions last so read-only folders can still be filled.
        fs::set_permissions(dst, meta.permissions()).map_err(|e| ctx(dst, e))
    } else if kind.is_file() {
        // fs::copy keeps the permission bits.
        fs::copy(src, dst).map(|_| ()).map_err(|e| ctx(src, e))
    } else {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!(
                "{}: sockets, pipes and devices are not copied",
                display_name(src)
            ),
        ))
    }
}

/// Move by renaming; across filesystems fall back to copy-then-delete.
pub fn move_path(src: &Path, dst: &Path) -> io::Result<()> {
    match fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(e) if e.raw_os_error() == Some(EXDEV) => {
            if let Err(e) = copy_path(src, dst) {
                let _ = delete_path(dst); // don't leave a half-copied mess
                return Err(e);
            }
            delete_path(src)
        }
        Err(e) => Err(ctx(src, e)),
    }
}

/// Permanently delete. A symlink is removed itself, never followed, and
/// `remove_dir_all` does not follow links inside folders either.
pub fn delete_path(path: &Path) -> io::Result<()> {
    let meta = fs::symlink_metadata(path).map_err(|e| ctx(path, e))?;
    if meta.file_type().is_dir() {
        fs::remove_dir_all(path).map_err(|e| ctx(path, e))
    } else {
        fs::remove_file(path).map_err(|e| ctx(path, e))
    }
}

pub fn validate_name(name: &str) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("The name can't be empty".into());
    }
    if name == "." || name == ".." {
        return Err("'.' and '..' are not valid names".into());
    }
    if name.contains('/') || name.contains('\0') {
        return Err("Names can't contain '/'".into());
    }
    if name.len() > 255 {
        return Err("The name is too long (255 bytes max)".into());
    }
    Ok(())
}

/// Rename in place. Refuses to replace an existing item.
pub fn rename_path(src: &Path, new_name: &str) -> Result<PathBuf, String> {
    validate_name(new_name)?;
    let dst = src.with_file_name(new_name);
    if dst == src {
        return Ok(dst);
    }
    if exists(&dst) {
        return Err(format!("'{new_name}' already exists here"));
    }
    fs::rename(src, &dst).map_err(|e| e.to_string())?;
    Ok(dst)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn write(p: &Path, s: &str) {
        fs::write(p, s).unwrap();
    }

    #[test]
    fn copy_is_recursive_and_keeps_permissions() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("src");
        fs::create_dir_all(src.join("sub")).unwrap();
        write(&src.join("a.txt"), "hello");
        write(&src.join("sub/run.sh"), "#!/bin/sh");
        fs::set_permissions(src.join("sub/run.sh"), fs::Permissions::from_mode(0o750)).unwrap();

        let dst = d.path().join("dst");
        copy_path(&src, &dst).unwrap();
        assert_eq!(fs::read_to_string(dst.join("a.txt")).unwrap(), "hello");
        let mode = fs::metadata(dst.join("sub/run.sh"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o750);
        assert!(src.join("a.txt").exists(), "source must be untouched");
    }

    #[test]
    fn copy_keeps_symlinks_as_links() {
        let d = tempfile::tempdir().unwrap();
        let outside = d.path().join("outside");
        fs::create_dir(&outside).unwrap();
        write(&outside.join("keep.txt"), "x");
        let src = d.path().join("src");
        fs::create_dir(&src).unwrap();
        symlink(&outside, src.join("link")).unwrap();

        let dst = d.path().join("dst");
        copy_path(&src, &dst).unwrap();
        let meta = fs::symlink_metadata(dst.join("link")).unwrap();
        assert!(meta.file_type().is_symlink());
        assert_eq!(fs::read_link(dst.join("link")).unwrap(), outside);
    }

    #[test]
    fn delete_does_not_follow_symlinks() {
        let d = tempfile::tempdir().unwrap();
        let outside = d.path().join("outside");
        fs::create_dir(&outside).unwrap();
        write(&outside.join("precious.txt"), "keep me");

        let victim = d.path().join("victim");
        fs::create_dir(&victim).unwrap();
        symlink(&outside, victim.join("link-to-outside")).unwrap();
        write(&victim.join("file"), "x");

        delete_path(&victim).unwrap();
        assert!(!victim.exists());
        assert!(
            outside.join("precious.txt").exists(),
            "link target was followed!"
        );

        // Deleting a symlink directly removes only the link.
        symlink(&outside, d.path().join("l2")).unwrap();
        delete_path(&d.path().join("l2")).unwrap();
        assert!(outside.exists());
    }

    #[test]
    fn delete_reports_missing_path() {
        let d = tempfile::tempdir().unwrap();
        assert!(delete_path(&d.path().join("nope")).is_err());
    }

    #[test]
    fn move_renames_and_removes_source() {
        let d = tempfile::tempdir().unwrap();
        write(&d.path().join("a"), "1");
        move_path(&d.path().join("a"), &d.path().join("b")).unwrap();
        assert!(!d.path().join("a").exists());
        assert_eq!(fs::read_to_string(d.path().join("b")).unwrap(), "1");
    }

    #[test]
    fn plan_flags_overwrites() {
        let d = tempfile::tempdir().unwrap();
        let (a, b) = (d.path().join("a"), d.path().join("b"));
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        write(&a.join("one"), "1");
        write(&a.join("two"), "2");
        write(&b.join("two"), "old");

        let plan = plan_paste(&[a.join("one"), a.join("two")], &b, Op::Copy);
        assert!(plan.problems.is_empty());
        assert_eq!(plan.items.len(), 2);
        assert!(!plan.items[0].overwrite);
        assert!(plan.items[1].overwrite);

        // Planning must not touch anything.
        assert_eq!(fs::read_to_string(b.join("two")).unwrap(), "old");
        assert!(!b.join("one").exists());

        let report = execute(Op::Copy, &plan);
        assert_eq!(report.done, 2);
        assert!(report.errors.is_empty());
        assert_eq!(fs::read_to_string(b.join("two")).unwrap(), "2");
    }

    #[test]
    fn plan_refuses_folder_into_itself() {
        let d = tempfile::tempdir().unwrap();
        let a = d.path().join("a");
        fs::create_dir_all(a.join("inner")).unwrap();
        for op in [Op::Copy, Op::Move] {
            let plan = plan_paste(&[a.clone()], &a.join("inner"), op);
            assert!(plan.items.is_empty());
            assert_eq!(plan.problems.len(), 1);
            assert!(plan.problems[0].contains("into itself"));
        }
    }

    #[test]
    fn plan_through_symlinked_dir_is_still_caught() {
        let d = tempfile::tempdir().unwrap();
        let a = d.path().join("a");
        fs::create_dir_all(&a).unwrap();
        let alias = d.path().join("alias");
        symlink(&a, &alias).unwrap();
        let plan = plan_paste(&[a.clone()], &alias, Op::Move);
        assert!(plan.items.is_empty());
        assert_eq!(plan.problems.len(), 1);
    }

    #[test]
    fn copy_into_same_folder_makes_a_copy_name() {
        let d = tempfile::tempdir().unwrap();
        write(&d.path().join("f.txt"), "x");
        let plan = plan_paste(&[d.path().join("f.txt")], d.path(), Op::Copy);
        assert_eq!(plan.items.len(), 1);
        assert!(!plan.items[0].overwrite);
        assert_eq!(display_name(&plan.items[0].dst), "f.txt copy");
        execute(Op::Copy, &plan);
        let again = plan_paste(&[d.path().join("f.txt")], d.path(), Op::Copy);
        assert_eq!(display_name(&again.items[0].dst), "f.txt copy 2");
    }

    #[test]
    fn move_into_same_folder_is_reported() {
        let d = tempfile::tempdir().unwrap();
        write(&d.path().join("f"), "x");
        let plan = plan_paste(&[d.path().join("f")], d.path(), Op::Move);
        assert!(plan.items.is_empty());
        assert_eq!(plan.problems.len(), 1);
    }

    #[test]
    fn overwrite_replaces_folders_with_files() {
        let d = tempfile::tempdir().unwrap();
        fs::create_dir_all(d.path().join("dest/thing/deep")).unwrap();
        write(&d.path().join("thing"), "file");
        let plan = plan_paste(&[d.path().join("thing")], &d.path().join("dest"), Op::Move);
        assert!(plan.items[0].overwrite);
        let r = execute(Op::Move, &plan);
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        assert_eq!(
            fs::read_to_string(d.path().join("dest/thing")).unwrap(),
            "file"
        );
        assert!(!d.path().join("thing").exists());
    }

    #[test]
    fn execute_reports_failures_and_continues() {
        let d = tempfile::tempdir().unwrap();
        write(&d.path().join("ok"), "1");
        let dest = d.path().join("dest");
        fs::create_dir(&dest).unwrap();
        let mut plan = plan_paste(&[d.path().join("ok")], &dest, Op::Copy);
        // Sabotage: a source that disappears between planning and running.
        plan.items.insert(
            0,
            PasteItem {
                src: d.path().join("ghost"),
                dst: dest.join("ghost"),
                overwrite: false,
            },
        );
        let r = execute(Op::Copy, &plan);
        assert_eq!(r.done, 1);
        assert_eq!(r.errors.len(), 1);
        assert!(r.errors[0].contains("ghost"));
    }

    #[test]
    fn rename_rules() {
        let d = tempfile::tempdir().unwrap();
        write(&d.path().join("a"), "");
        write(&d.path().join("taken"), "");

        assert!(rename_path(&d.path().join("a"), "").is_err());
        assert!(rename_path(&d.path().join("a"), "..").is_err());
        assert!(rename_path(&d.path().join("a"), "x/y").is_err());
        assert!(rename_path(&d.path().join("a"), "taken")
            .unwrap_err()
            .contains("already exists"));
        assert!(d.path().join("a").exists());

        let new = rename_path(&d.path().join("a"), "b").unwrap();
        assert_eq!(new, d.path().join("b"));
        assert!(new.exists() && !d.path().join("a").exists());
        // Same name is a harmless no-op.
        assert!(rename_path(&new, "b").is_ok());
    }
}
