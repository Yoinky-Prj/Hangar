//! Copy an existing folder (from outside the workspace) into a section.
//! The source is never modified.

use crate::error::{io_ctx, msg, AppResult};
use crate::git::{remove_dir_all_force, JobProgress, ProgressFn};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use walkdir::{DirEntry, WalkDir};

/// Folders that are rebuilt by package managers and break when moved anyway.
const REGENERABLE: &[&str] = &[
    "node_modules", "__pycache__", ".pytest_cache", ".mypy_cache", ".ruff_cache", ".next", ".turbo", ".parcel-cache",
];

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ImportInspection {
    pub path: String,
    pub folder: String,
    pub is_git: bool,
    pub remote: Option<crate::naming::RepoRef>,
    pub suggested_name: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub files: u64,
    pub bytes: u64,
    pub skipped_dirs: Vec<String>,
    pub skipped_links: Vec<String>,
}

pub fn inspect(src: &Path) -> AppResult<ImportInspection> {
    if !src.is_dir() {
        return Err(msg(format!("{} is not a folder.", src.display())));
    }
    let folder = src.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let is_git = src.join(".git").exists();
    let remote = if is_git { crate::meta::detect_remote(src) } else { None };
    let suggested_name = match &remote {
        Some(r) => r.folder_name(),
        None => crate::naming::sanitize_component(&folder),
    };
    Ok(ImportInspection { path: src.to_string_lossy().to_string(), folder, is_git, remote, suggested_name })
}

/// Is this directory something we skip when "skip regenerable" is on?
fn is_regenerable(e: &DirEntry) -> bool {
    if !e.file_type().is_dir() {
        return false;
    }
    let name = e.file_name().to_string_lossy();
    if REGENERABLE.contains(&name.as_ref()) {
        return true;
    }
    // Python virtualenvs store absolute paths and must be recreated after a move.
    if e.path().join("pyvenv.cfg").is_file() {
        return true;
    }
    // Rust build output, only when it sits next to a Cargo.toml.
    name == "target" && e.path().parent().is_some_and(|p| p.join("Cargo.toml").is_file())
}

pub fn copy_project(
    src: &Path,
    dest: &Path,
    skip_regenerable: bool,
    cancel: Arc<AtomicBool>,
    on: ProgressFn,
) -> AppResult<ImportReport> {
    if dest.exists() {
        return Err(msg(format!("{} already exists.", dest.display())));
    }
    let canon_src = fs::canonicalize(src).map_err(|e| io_ctx(e, "read", src))?;
    if let Some(parent) = dest.parent() {
        if let Ok(cp) = fs::canonicalize(parent) {
            if cp.starts_with(&canon_src) {
                return Err(msg("Can't import a folder into itself."));
            }
        }
    }

    let skip = move |e: &DirEntry| skip_regenerable && e.depth() > 0 && is_regenerable(e);

    // Pass 1: size the job so progress means something, and note what is skipped.
    let mut total_files = 0u64;
    let mut skipped_dirs = Vec::new();
    {
        let it = WalkDir::new(src).follow_links(false).into_iter().filter_entry(|e| {
            if skip(e) {
                skipped_dirs.push(rel(src, e.path()));
                false
            } else {
                true
            }
        });
        for e in it.flatten() {
            if cancel.load(Ordering::Relaxed) {
                return Err(msg("Cancelled."));
            }
            if e.file_type().is_file() {
                total_files += 1;
            }
        }
    }

    let mut report = ImportReport { files: 0, bytes: 0, skipped_dirs, skipped_links: Vec::new() };

    let result = (|| -> AppResult<()> {
        fs::create_dir_all(dest).map_err(|e| io_ctx(e, "create", dest))?;
        let mut last = Instant::now() - Duration::from_secs(1);
        for e in WalkDir::new(src).follow_links(false).into_iter().filter_entry(|e| !skip(e)) {
            if cancel.load(Ordering::Relaxed) {
                return Err(msg("Cancelled."));
            }
            let e = e.map_err(|err| msg(format!("Could not read {}: {err}", src.display())))?;
            let target: PathBuf = dest.join(e.path().strip_prefix(src).unwrap_or(e.path()));
            let ft = e.file_type();
            if ft.is_symlink() {
                report.skipped_links.push(rel(src, e.path()));
            } else if ft.is_dir() {
                fs::create_dir_all(&target).map_err(|err| io_ctx(err, "create", &target))?;
            } else if ft.is_file() {
                let n = fs::copy(e.path(), &target).map_err(|err| io_ctx(err, "copy", e.path()))?;
                report.files += 1;
                report.bytes += n;
                if last.elapsed() >= Duration::from_millis(120) || report.files == total_files {
                    last = Instant::now();
                    on(JobProgress {
                        stage: "copying".into(),
                        current: report.files,
                        total: total_files,
                        bytes: report.bytes,
                        detail: None,
                    });
                }
            }
        }
        Ok(())
    })();

    if let Err(e) = result {
        let _ = remove_dir_all_force(dest);
        return Err(e);
    }
    Ok(report)
}

fn rel(base: &Path, p: &Path) -> String {
    p.strip_prefix(base).unwrap_or(p).to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_and_skips_regenerable_dirs() {
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("proj");
        fs::create_dir_all(src.join("src")).unwrap();
        fs::create_dir_all(src.join("node_modules/pkg")).unwrap();
        fs::create_dir_all(src.join(".venv")).unwrap();
        fs::write(src.join(".venv/pyvenv.cfg"), "home = x").unwrap();
        fs::create_dir_all(src.join("data/venv-notes")).unwrap();
        fs::write(src.join("src/main.ts"), "hi").unwrap();
        fs::write(src.join("node_modules/pkg/index.js"), "x").unwrap();
        fs::write(src.join("data/venv-notes/n.txt"), "keep").unwrap();

        let dest = t.path().join("ws/Projects/proj");
        fs::create_dir_all(dest.parent().unwrap()).unwrap();
        let r = copy_project(&src, &dest, true, Arc::new(AtomicBool::new(false)), Arc::new(|_| {})).unwrap();

        assert_eq!(fs::read_to_string(dest.join("src/main.ts")).unwrap(), "hi");
        assert!(dest.join("data/venv-notes/n.txt").exists());
        assert!(!dest.join("node_modules").exists());
        assert!(!dest.join(".venv").exists());
        assert_eq!(r.files, 2);
        assert!(r.skipped_dirs.contains(&"node_modules".to_string()));
        assert!(r.skipped_dirs.contains(&".venv".to_string()));
        // Source untouched.
        assert!(src.join("node_modules/pkg/index.js").exists());
    }

    #[test]
    fn refuses_existing_destination_and_self_import() {
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("p");
        fs::create_dir_all(&src).unwrap();
        let noop: ProgressFn = Arc::new(|_| {});
        assert!(copy_project(&src, &src, false, Arc::new(AtomicBool::new(false)), noop.clone()).is_err());
        let inside = src.join("sub/p2");
        fs::create_dir_all(src.join("sub")).unwrap();
        assert!(copy_project(&src, &inside, false, Arc::new(AtomicBool::new(false)), noop).is_err());
    }
}
