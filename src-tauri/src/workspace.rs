//! A workspace is a folder with `.hangar/workspace.json` plus the three
//! section folders: `Projects`, `Reference` and `.archive`.

use crate::error::{io_ctx, msg, AppResult};
use crate::meta::{self, write_json_atomic, ProjectMeta, Section, Source, SourceKind};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const WS_DIR: &str = ".hangar";
const WS_FILE: &str = "workspace.json";
const INDEX_FILE: &str = "index.json";

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    pub id: String,
    pub name: String,
    pub color: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceConfig {
    pub version: u32,
    pub name: String,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub categories: Vec<Category>,
}

/// Backup of every project's metadata, so a deleted-then-recloned repo gets
/// its id, categories and section back.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Index {
    #[serde(default)]
    pub projects: BTreeMap<String, IndexEntry>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct IndexEntry {
    /// Path relative to the workspace root, e.g. `Projects/Yoinky-Prj__Main-Web`.
    pub folder: String,
    pub meta: ProjectMeta,
    #[serde(default)]
    pub missing: bool,
    pub updated_at: DateTime<Utc>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FolderInspection {
    pub exists: bool,
    pub is_workspace: bool,
    pub entry_count: usize,
    pub name: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProjectEntry {
    pub folder: String,
    pub path: String,
    pub section: Section,
    pub managed: bool,
    pub is_git: bool,
    pub meta: Option<ProjectMeta>,
    pub modified: Option<DateTime<Utc>>,
}

fn ws_dir(root: &Path) -> PathBuf {
    root.join(WS_DIR)
}

pub fn section_dir(root: &Path, s: Section) -> PathBuf {
    root.join(s.dir_name())
}

pub fn is_workspace(root: &Path) -> bool {
    ws_dir(root).join(WS_FILE).is_file()
}

pub fn require_workspace(root: &Path) -> AppResult<()> {
    if is_workspace(root) {
        Ok(())
    } else {
        Err(msg(format!("{} is not a Hangar workspace.", root.display())))
    }
}

pub fn inspect_folder(root: &Path) -> FolderInspection {
    let exists = root.is_dir();
    let entry_count = if exists { fs::read_dir(root).map(|d| d.count()).unwrap_or(0) } else { 0 };
    FolderInspection {
        exists,
        is_workspace: is_workspace(root),
        entry_count,
        name: root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
    }
}

/// One entry of the workspace list: enough to show it without opening it.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceSummary {
    pub path: String,
    /// The workspace's own name, or the folder name if it isn't set up.
    pub name: String,
    pub exists: bool,
    pub is_workspace: bool,
    pub entry_count: usize,
    /// Project folders in Projects, Reference and .archive.
    pub projects: usize,
}

pub fn summarize(path: &str) -> WorkspaceSummary {
    let root = Path::new(path);
    let insp = inspect_folder(root);
    let (name, projects) = if insp.is_workspace {
        let name = load_config(root).ok().map(|c| c.name).filter(|n| !n.trim().is_empty());
        let count = Section::ALL
            .into_iter()
            .map(|s| {
                fs::read_dir(section_dir(root, s))
                    .map(|rd| rd.flatten().filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false)).count())
                    .unwrap_or(0)
            })
            .sum();
        (name, count)
    } else {
        (None, 0)
    };
    WorkspaceSummary {
        path: path.to_string(),
        name: name.unwrap_or(insp.name),
        exists: insp.exists,
        is_workspace: insp.is_workspace,
        entry_count: insp.entry_count,
        projects,
    }
}

pub fn load_config(root: &Path) -> AppResult<WorkspaceConfig> {
    require_workspace(root)?;
    let p = ws_dir(root).join(WS_FILE);
    let s = fs::read_to_string(&p).map_err(|e| io_ctx(e, "read", &p))?;
    Ok(serde_json::from_str(&s)?)
}

pub fn save_config(root: &Path, cfg: &WorkspaceConfig) -> AppResult<()> {
    write_json_atomic(&ws_dir(root).join(WS_FILE), cfg)
}

/// Create the workspace structure. Existing content is never touched.
pub fn init_workspace(root: &Path) -> AppResult<WorkspaceConfig> {
    fs::create_dir_all(root).map_err(|e| io_ctx(e, "create", root))?;
    for s in Section::ALL {
        let d = section_dir(root, s);
        fs::create_dir_all(&d).map_err(|e| io_ctx(e, "create", &d))?;
    }
    if is_workspace(root) {
        return load_config(root);
    }
    let cfg = WorkspaceConfig {
        version: 1,
        name: root.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Workspace".into()),
        created_at: Utc::now(),
        categories: Vec::new(),
    };
    save_config(root, &cfg)?;
    write_json_atomic(&ws_dir(root).join(INDEX_FILE), &Index::default())?;
    Ok(cfg)
}

pub fn load_index(root: &Path) -> Index {
    fs::read_to_string(ws_dir(root).join(INDEX_FILE))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_index(root: &Path, idx: &Index) -> AppResult<()> {
    write_json_atomic(&ws_dir(root).join(INDEX_FILE), idx)
}

fn rel_folder(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).unwrap_or(path).to_string_lossy().replace('\\', "/")
}

fn upsert_index(root: &Path, path: &Path, m: &ProjectMeta) -> AppResult<()> {
    let mut idx = load_index(root);
    idx.projects.insert(
        m.id.clone(),
        IndexEntry { folder: rel_folder(root, path), meta: m.clone(), missing: false, updated_at: Utc::now() },
    );
    save_index(root, &idx)
}

/// Write project metadata and mirror it into the workspace index.
pub fn save_project_meta(root: &Path, project: &Path, m: &ProjectMeta) -> AppResult<()> {
    meta::write_meta(project, m)?;
    upsert_index(root, project, m)
}

/// Find metadata of a project that used to be in this workspace (folder now gone)
/// and came from the same remote, so a re-clone can inherit it.
pub fn find_previous(root: &Path, remote_key: &str) -> Option<ProjectMeta> {
    load_index(root)
        .projects
        .into_values()
        .filter(|e| !root.join(&e.folder).exists())
        .filter(|e| e.meta.source.remote.as_ref().map(|r| r.key()) == Some(remote_key.to_string()))
        .max_by_key(|e| e.updated_at)
        .map(|e| e.meta)
}

fn entry_for(path: &Path, section: Section, meta: Option<ProjectMeta>) -> ProjectEntry {
    let modified = fs::metadata(path).and_then(|m| m.modified()).ok().map(DateTime::<Utc>::from);
    ProjectEntry {
        folder: path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
        path: path.to_string_lossy().to_string(),
        section,
        managed: meta.is_some(),
        is_git: path.join(".git").exists(),
        meta,
        modified,
    }
}

pub fn project_entry(root: &Path, path: &Path) -> AppResult<ProjectEntry> {
    let section = section_of(root, path)?;
    Ok(entry_for(path, section, meta::read_meta(path)?))
}

/// List every folder in the three sections. Metadata whose recorded section
/// disagrees with the folder's real location (moved by hand) is corrected.
pub fn scan(root: &Path) -> AppResult<Vec<ProjectEntry>> {
    require_workspace(root)?;
    let mut out = Vec::new();
    let mut idx = load_index(root);
    let mut seen = std::collections::HashSet::new();
    for s in Section::ALL {
        let dir = section_dir(root, s);
        let Ok(rd) = fs::read_dir(&dir) else { continue };
        for ent in rd.flatten() {
            let path = ent.path();
            if !ent.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let mut m = meta::read_meta(&path).unwrap_or(None);
            if let Some(mm) = m.as_mut() {
                if mm.section != s {
                    if s == Section::Archive && mm.archived_from.is_none() {
                        mm.archived_from = Some(mm.section);
                    }
                    mm.section = s;
                    let _ = meta::write_meta(&path, mm);
                }
                seen.insert(mm.id.clone());
                idx.projects.insert(
                    mm.id.clone(),
                    IndexEntry { folder: rel_folder(root, &path), meta: mm.clone(), missing: false, updated_at: Utc::now() },
                );
            }
            out.push(entry_for(&path, s, m));
        }
    }
    for (id, e) in idx.projects.iter_mut() {
        if !seen.contains(id) {
            e.missing = true;
        }
    }
    let _ = save_index(root, &idx);
    out.sort_by(|a, b| a.folder.to_lowercase().cmp(&b.folder.to_lowercase()));
    Ok(out)
}

/// Which section a project path lives in. Rejects anything outside the workspace.
pub fn section_of(root: &Path, path: &Path) -> AppResult<Section> {
    let parent = path.parent().ok_or_else(|| msg("Invalid project path."))?;
    let canon_parent = dunce(parent);
    for s in Section::ALL {
        if dunce(&section_dir(root, s)) == canon_parent {
            return Ok(s);
        }
    }
    Err(msg(format!("{} is not a project folder of this workspace.", path.display())))
}

/// Canonicalize without the `\\?\` prefix so paths compare cleanly on Windows.
fn dunce(p: &Path) -> PathBuf {
    let c = fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let s = c.to_string_lossy();
    PathBuf::from(s.strip_prefix(r"\\?\").unwrap_or(&s).to_lowercase())
}

/// Error if a folder with this name exists in any section.
pub fn ensure_name_free(root: &Path, folder: &str) -> AppResult<()> {
    for s in Section::ALL {
        if section_dir(root, s).join(folder).exists() {
            let where_ = if s == Section::Archive { ".archive".to_string() } else { s.dir_name().to_string() };
            return Err(msg(format!("\"{folder}\" already exists in {where_}.")));
        }
    }
    Ok(())
}

pub fn move_project(root: &Path, path: &Path, target: Section) -> AppResult<ProjectEntry> {
    require_workspace(root)?;
    let from = section_of(root, path)?;
    if from == target {
        return project_entry(root, path);
    }
    let folder = path.file_name().ok_or_else(|| msg("Invalid project path."))?;
    let dest = section_dir(root, target).join(folder);
    if dest.exists() {
        return Err(msg(format!("\"{}\" already exists in {}.", folder.to_string_lossy(), target.dir_name())));
    }
    let existing = meta::read_meta(path)?;
    fs::rename(path, &dest).map_err(|e| io_ctx(e, "move", path))?;

    // Moving an unmanaged folder adopts it, so archive can remember its origin.
    let mut m = existing.unwrap_or_else(|| {
        let remote = meta::detect_remote(&dest);
        ProjectMeta::new(
            from,
            Source { kind: SourceKind::Adopted, remote: remote.as_ref().map(Into::into), imported_from: None },
            Vec::new(),
        )
    });
    if target == Section::Archive {
        m.archived_from = Some(from);
        m.archived_at = Some(Utc::now());
    } else {
        m.archived_from = None;
        m.archived_at = None;
    }
    m.section = target;
    save_project_meta(root, &dest, &m)?;
    project_entry(root, &dest)
}

pub fn unarchive(root: &Path, path: &Path) -> AppResult<ProjectEntry> {
    let back = meta::read_meta(path)?.and_then(|m| m.archived_from).unwrap_or(Section::Projects);
    let back = if back == Section::Archive { Section::Projects } else { back };
    move_project(root, path, back)
}

/// Create metadata for a folder that was put into a section by hand.
pub fn adopt(root: &Path, path: &Path) -> AppResult<ProjectEntry> {
    require_workspace(root)?;
    let section = section_of(root, path)?;
    if meta::read_meta(path)?.is_some() {
        return project_entry(root, path);
    }
    let remote = meta::detect_remote(path);
    let prev = remote.as_ref().and_then(|r| find_previous(root, &r.key()));
    let m = match prev {
        Some(mut p) => {
            p.section = section;
            p
        }
        None => ProjectMeta::new(
            section,
            Source { kind: SourceKind::Adopted, remote: remote.as_ref().map(Into::into), imported_from: None },
            Vec::new(),
        ),
    };
    save_project_meta(root, path, &m)?;
    project_entry(root, path)
}

pub fn set_categories(root: &Path, path: &Path, ids: Vec<String>) -> AppResult<ProjectEntry> {
    require_workspace(root)?;
    section_of(root, path)?;
    let mut m = meta::read_meta(path)?.ok_or_else(|| msg("Adopt this folder first, then set its categories."))?;
    m.categories = ids;
    save_project_meta(root, path, &m)?;
    project_entry(root, path)
}

/// Set or clear (empty) a project's display name. The folder is not renamed.
pub fn set_name(root: &Path, path: &Path, name: Option<&str>) -> AppResult<ProjectEntry> {
    require_workspace(root)?;
    section_of(root, path)?;
    let mut m = meta::read_meta(path)?.ok_or_else(|| msg("Adopt this folder first, then name it."))?;
    m.name = meta::clean_name(name);
    save_project_meta(root, path, &m)?;
    project_entry(root, path)
}

pub fn add_category(root: &Path, name: &str, color: &str) -> AppResult<WorkspaceConfig> {
    let mut cfg = load_config(root)?;
    let name = name.trim();
    if name.is_empty() {
        return Err(msg("Category name can't be empty."));
    }
    if cfg.categories.iter().any(|c| c.name.eq_ignore_ascii_case(name)) {
        return Err(msg(format!("A category called \"{name}\" already exists.")));
    }
    cfg.categories.push(Category { id: uuid::Uuid::new_v4().to_string(), name: name.into(), color: color.into() });
    save_config(root, &cfg)?;
    Ok(cfg)
}

pub fn update_category(root: &Path, id: &str, name: &str, color: &str) -> AppResult<WorkspaceConfig> {
    let mut cfg = load_config(root)?;
    let name = name.trim();
    if name.is_empty() {
        return Err(msg("Category name can't be empty."));
    }
    if cfg.categories.iter().any(|c| c.id != id && c.name.eq_ignore_ascii_case(name)) {
        return Err(msg(format!("A category called \"{name}\" already exists.")));
    }
    let c = cfg.categories.iter_mut().find(|c| c.id == id).ok_or_else(|| msg("Category not found."))?;
    c.name = name.into();
    c.color = color.into();
    save_config(root, &cfg)?;
    Ok(cfg)
}

/// Delete a category and remove it from every project that uses it.
pub fn delete_category(root: &Path, id: &str) -> AppResult<WorkspaceConfig> {
    let mut cfg = load_config(root)?;
    cfg.categories.retain(|c| c.id != id);
    save_config(root, &cfg)?;
    for e in scan(root)? {
        if let Some(mut m) = e.meta {
            if m.categories.iter().any(|c| c == id) {
                m.categories.retain(|c| c != id);
                save_project_meta(root, Path::new(&e.path), &m)?;
            }
        }
    }
    Ok(cfg)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws() -> (tempfile::TempDir, PathBuf) {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("Workspaces");
        init_workspace(&root).unwrap();
        (t, root)
    }

    #[test]
    fn init_creates_structure_and_keeps_existing_files() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("W");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("keep.txt"), "x").unwrap();
        init_workspace(&root).unwrap();
        assert!(is_workspace(&root));
        for s in Section::ALL {
            assert!(section_dir(&root, s).is_dir());
        }
        assert_eq!(fs::read_to_string(root.join("keep.txt")).unwrap(), "x");
        // Re-init is a no-op that returns the same config.
        let a = load_config(&root).unwrap();
        let b = init_workspace(&root).unwrap();
        assert_eq!(a.created_at, b.created_at);
    }

    #[test]
    fn adopt_archive_unarchive_roundtrip() {
        let (_t, root) = ws();
        let p = section_dir(&root, Section::Reference).join("thing");
        fs::create_dir_all(&p).unwrap();

        let e = scan(&root).unwrap();
        assert_eq!(e.len(), 1);
        assert!(!e[0].managed);

        let adopted = adopt(&root, &p).unwrap();
        assert!(adopted.managed);

        let archived = move_project(&root, &p, Section::Archive).unwrap();
        assert_eq!(archived.section, Section::Archive);
        let m = archived.meta.unwrap();
        assert_eq!(m.archived_from, Some(Section::Reference));

        let back = unarchive(&root, Path::new(&archived.path)).unwrap();
        assert_eq!(back.section, Section::Reference);
        assert!(back.meta.unwrap().archived_from.is_none());
    }

    #[test]
    fn rejects_paths_outside_workspace() {
        let (t, root) = ws();
        let outside = t.path().join("elsewhere");
        fs::create_dir_all(&outside).unwrap();
        assert!(move_project(&root, &outside, Section::Archive).is_err());
        assert!(adopt(&root, &outside).is_err());
    }

    #[test]
    fn hand_moved_folder_gets_section_fixed() {
        let (_t, root) = ws();
        let p = section_dir(&root, Section::Projects).join("a");
        fs::create_dir_all(&p).unwrap();
        adopt(&root, &p).unwrap();
        let dest = section_dir(&root, Section::Archive).join("a");
        fs::rename(&p, &dest).unwrap();
        let e = scan(&root).unwrap();
        let m = e[0].meta.as_ref().unwrap();
        assert_eq!(m.section, Section::Archive);
        assert_eq!(m.archived_from, Some(Section::Projects));
    }

    #[test]
    fn display_name_is_set_trimmed_and_cleared() {
        let (_t, root) = ws();
        let p = section_dir(&root, Section::Projects).join("o__r");
        fs::create_dir_all(&p).unwrap();
        adopt(&root, &p).unwrap();
        let e = set_name(&root, &p, Some("  Discord Bot  ")).unwrap();
        assert_eq!(e.meta.unwrap().name.as_deref(), Some("Discord Bot"));
        assert_eq!(e.folder, "o__r", "folder is not renamed");
        let e = set_name(&root, &p, Some("   ")).unwrap();
        assert!(e.meta.unwrap().name.is_none());
    }

    #[test]
    fn deleting_a_category_removes_it_from_projects() {
        let (_t, root) = ws();
        let cfg = add_category(&root, "Bots", "#f00").unwrap();
        let id = cfg.categories[0].id.clone();
        assert!(add_category(&root, "bots", "#0f0").is_err());
        let p = section_dir(&root, Section::Projects).join("x");
        fs::create_dir_all(&p).unwrap();
        adopt(&root, &p).unwrap();
        set_categories(&root, &p, vec![id.clone()]).unwrap();
        delete_category(&root, &id).unwrap();
        assert!(meta::read_meta(&p).unwrap().unwrap().categories.is_empty());
    }

    #[test]
    fn previous_metadata_is_found_after_folder_removed() {
        let (_t, root) = ws();
        let p = section_dir(&root, Section::Reference).join("o__r");
        fs::create_dir_all(&p).unwrap();
        let remote = crate::naming::parse_repo_input("o/r").unwrap();
        let mut m = ProjectMeta::new(
            Section::Reference,
            Source { kind: SourceKind::Clone, remote: Some((&remote).into()), imported_from: None },
            vec!["cat".into()],
        );
        m.categories = vec!["cat".into()];
        save_project_meta(&root, &p, &m).unwrap();
        assert!(find_previous(&root, &remote.key()).is_none()); // still present
        fs::remove_dir_all(&p).unwrap();
        let prev = find_previous(&root, &remote.key()).unwrap();
        assert_eq!(prev.id, m.id);
        assert_eq!(prev.categories, vec!["cat".to_string()]);
    }
}
