//! Per-project metadata stored in `<project>/.hangar/project.json`.
//! It is local-only: Hangar adds `/.hangar/` to the repo's `.git/info/exclude`.

use crate::error::{io_ctx, AppResult};
use crate::naming::RepoRef;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const META_DIR: &str = ".hangar";
pub const META_FILE: &str = "project.json";
const EXCLUDE_LINE: &str = "/.hangar/";

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Section {
    Projects,
    Reference,
    Archive,
}

impl Section {
    pub const ALL: [Section; 3] = [Section::Projects, Section::Reference, Section::Archive];

    pub fn dir_name(self) -> &'static str {
        match self {
            Section::Projects => "Projects",
            Section::Reference => "Reference",
            Section::Archive => ".archive",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    Clone,
    New,
    Import,
    Adopted,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RemoteInfo {
    pub host: String,
    pub owner: String,
    pub repo: String,
    pub url: String,
}

impl From<&RepoRef> for RemoteInfo {
    fn from(r: &RepoRef) -> Self {
        RemoteInfo { host: r.host.clone(), owner: r.owner.clone(), repo: r.repo.clone(), url: r.web_url() }
    }
}

impl RemoteInfo {
    pub fn key(&self) -> String {
        format!("{}/{}/{}", self.host, self.owner, self.repo).to_lowercase()
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    pub kind: SourceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote: Option<RemoteInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub imported_from: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProjectMeta {
    pub version: u32,
    pub id: String,
    /// Display name. Unset = show `owner/repo` (or the folder name).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub section: Section,
    /// Where an archived project came from, so unarchiving puts it back.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_from: Option<Section>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archived_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub categories: Vec<String>,
    pub source: Source,
    pub created_at: DateTime<Utc>,
}

impl ProjectMeta {
    pub fn new(section: Section, source: Source, categories: Vec<String>) -> Self {
        ProjectMeta {
            version: 1,
            id: uuid::Uuid::new_v4().to_string(),
            name: None,
            section,
            archived_from: None,
            archived_at: None,
            categories,
            source,
            created_at: Utc::now(),
        }
    }
}

/// Trimmed display name; empty means "no name".
pub fn clean_name(name: Option<&str>) -> Option<String> {
    name.map(str::trim).filter(|n| !n.is_empty()).map(String::from)
}

pub fn meta_path(project: &Path) -> PathBuf {
    project.join(META_DIR).join(META_FILE)
}

pub fn read_meta(project: &Path) -> AppResult<Option<ProjectMeta>> {
    let p = meta_path(project);
    match fs::read_to_string(&p) {
        Ok(s) => Ok(Some(serde_json::from_str(&s)?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io_ctx(e, "read", &p)),
    }
}

/// Write via a temp file + rename so a crash never leaves half a JSON file.
pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> AppResult<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| io_ctx(e, "create", dir))?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(value)?).map_err(|e| io_ctx(e, "write", &tmp))?;
    fs::rename(&tmp, path).map_err(|e| io_ctx(e, "save", path))?;
    Ok(())
}

pub fn write_meta(project: &Path, meta: &ProjectMeta) -> AppResult<()> {
    write_json_atomic(&meta_path(project), meta)?;
    ensure_git_exclude(project)?;
    Ok(())
}

/// Keep `.hangar/` out of `git status` without committing anything.
/// Works for normal repos, worktrees and submodules (whose `.git` is a file).
pub fn ensure_git_exclude(project: &Path) -> AppResult<()> {
    if !project.join(".git").exists() {
        return Ok(());
    }
    let repo = match git2::Repository::open(project) {
        Ok(r) => r,
        Err(_) => return Ok(()), // broken repo: leave it alone
    };
    let info = repo.commondir().join("info");
    let exclude = info.join("exclude");
    let current = fs::read_to_string(&exclude).unwrap_or_default();
    if current.lines().any(|l| l.trim() == EXCLUDE_LINE) {
        return Ok(());
    }
    fs::create_dir_all(&info).map_err(|e| io_ctx(e, "create", &info))?;
    let mut next = current;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str("# Hangar metadata (local only)\n");
    next.push_str(EXCLUDE_LINE);
    next.push('\n');
    fs::write(&exclude, next).map_err(|e| io_ctx(e, "write", &exclude))?;
    Ok(())
}

/// The `origin` remote (or the first remote) of a repo, parsed into owner/repo.
pub fn detect_remote(project: &Path) -> Option<RepoRef> {
    let repo = git2::Repository::open(project).ok()?;
    let remote = repo.find_remote("origin").ok().or_else(|| {
        let names = repo.remotes().ok()?;
        let first = names.iter().flatten().flatten().next()?.to_string();
        repo.find_remote(&first).ok()
    })?;
    crate::naming::parse_repo_input(remote.url().ok()?)
}
