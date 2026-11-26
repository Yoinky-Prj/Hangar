//! Tauri commands: the API the React UI calls.

use crate::branches;
use crate::error::{msg, AppError, AppResult};
use crate::git::{self, GitStatus, JobProgress, ProgressFn};
use crate::github::{self, Account, DeviceCode, GhRepo, PollResult};
use crate::importer::{self, ImportInspection, ImportReport};
use crate::meta::{clean_name, ProjectMeta, RemoteInfo, Section, Source, SourceKind};
use crate::naming::{self, RepoRef};
use crate::settings::{self, Settings};
use crate::system::{self, Environment};
use crate::workspace::{self, FolderInspection, ProjectEntry, WorkspaceConfig, WorkspaceSummary};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_opener::OpenerExt;

type R<T> = Result<T, AppError>;

pub struct AppState {
    http: reqwest::Client,
    jobs: Mutex<HashMap<String, Arc<AtomicBool>>>,
    /// Serializes GitHub token renewal (a refresh token only works once).
    gh_lock: tokio::sync::Mutex<()>,
}

impl AppState {
    pub fn new() -> Self {
        AppState { http: github::client(), jobs: Mutex::new(HashMap::new()), gh_lock: tokio::sync::Mutex::new(()) }
    }

    /// A clone or import is running (they leave half-copied folders if cut off).
    pub fn busy(&self) -> bool {
        !self.jobs.lock().unwrap().is_empty()
    }

    async fn gh_token(&self) -> AppResult<Option<github::StoredToken>> {
        github::valid_token(&self.http, &self.gh_lock).await
    }

    fn start_job(&self, id: &str) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        self.jobs.lock().unwrap().insert(id.to_string(), flag.clone());
        flag
    }

    fn end_job(&self, id: &str) {
        self.jobs.lock().unwrap().remove(id);
    }
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> AppResult<T> + Send + 'static) -> AppResult<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| msg(format!("Background task failed: {e}")))?
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct JobEvent {
    job_id: String,
    #[serde(flatten)]
    progress: JobProgress,
}

fn emitter(app: &AppHandle, job_id: &str) -> ProgressFn {
    let app = app.clone();
    let job_id = job_id.to_string();
    Arc::new(move |p| {
        let _ = app.emit("job-progress", JobEvent { job_id: job_id.clone(), progress: p });
    })
}

fn not_archive(s: Section) -> AppResult<()> {
    if s == Section::Archive {
        Err(msg("New projects go into Projects or Reference. Archive them afterwards."))
    } else {
        Ok(())
    }
}

// ---------- settings & environment ----------

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Settings {
    settings::load(&app)
}

#[tauri::command]
pub fn set_github_client_id(app: AppHandle, client_id: String) -> R<Settings> {
    settings::set_client_id(&app, &client_id)
}

/// The workspace list, with each one's name, state and project count.
#[tauri::command]
pub async fn list_workspaces(app: AppHandle) -> R<Vec<WorkspaceSummary>> {
    let paths = settings::load(&app).workspaces;
    blocking(move || Ok(paths.iter().map(|p| workspace::summarize(p)).collect())).await
}

#[tauri::command]
pub fn forget_workspace(app: AppHandle, path: String) -> R<Settings> {
    settings::forget_workspace(&app, &path)
}

#[tauri::command]
pub fn environment() -> Environment {
    system::detect()
}

// ---------- workspace ----------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceView {
    root: String,
    config: WorkspaceConfig,
}

#[tauri::command]
pub fn inspect_folder(path: String) -> FolderInspection {
    workspace::inspect_folder(Path::new(&path))
}

#[tauri::command]
pub fn init_workspace(app: AppHandle, path: String) -> R<WorkspaceView> {
    let config = workspace::init_workspace(Path::new(&path))?;
    settings::remember_workspace(&app, &path)?;
    Ok(WorkspaceView { root: path, config })
}

#[tauri::command]
pub fn open_workspace(app: AppHandle, path: String) -> R<WorkspaceView> {
    let config = workspace::load_config(Path::new(&path))?;
    settings::remember_workspace(&app, &path)?;
    Ok(WorkspaceView { root: path, config })
}

#[tauri::command]
pub async fn scan_projects(root: String) -> R<Vec<ProjectEntry>> {
    blocking(move || workspace::scan(Path::new(&root))).await
}

#[tauri::command]
pub async fn git_status(path: String) -> R<GitStatus> {
    blocking(move || git::status(Path::new(&path))).await
}

#[tauri::command]
pub async fn git_submodules(path: String) -> R<Vec<git::SubmoduleInfo>> {
    blocking(move || git::submodules(Path::new(&path))).await
}

#[tauri::command]
pub async fn git_fetch(state: State<'_, AppState>, path: String) -> R<git::FetchOutcome> {
    let token = state.gh_token().await.ok().flatten().map(|t| t.access_token);
    blocking(move || git::fetch_all(Path::new(&path), token)).await
}

/// Turn a "latest commit only" clone into a full one (all branches and history).
#[tauri::command]
pub async fn git_full_history(state: State<'_, AppState>, path: String) -> R<()> {
    let token = state.gh_token().await.ok().flatten().map(|t| t.access_token);
    blocking(move || git::full_history(Path::new(&path), token)).await
}

#[tauri::command]
pub async fn git_pull(state: State<'_, AppState>, path: String) -> R<git::PullOutcome> {
    let token = state.gh_token().await.ok().flatten().map(|t| t.access_token);
    blocking(move || git::pull_ff(Path::new(&path), token)).await
}

// ---------- branches ----------

#[tauri::command]
pub async fn git_branches(path: String) -> R<branches::BranchList> {
    blocking(move || branches::list(Path::new(&path))).await
}

#[tauri::command]
pub async fn git_switch_branch(path: String, name: String) -> R<()> {
    blocking(move || branches::switch(Path::new(&path), &name)).await
}

#[tauri::command]
pub async fn git_checkout_remote(path: String, remote_branch: String) -> R<String> {
    blocking(move || branches::checkout_remote(Path::new(&path), &remote_branch)).await
}

#[tauri::command]
pub async fn git_create_branch(path: String, name: String, from: Option<String>, switch: bool) -> R<String> {
    blocking(move || branches::create(Path::new(&path), &name, from.as_deref(), switch)).await
}

#[tauri::command]
pub async fn git_update_branch(state: State<'_, AppState>, path: String, name: String) -> R<git::PullOutcome> {
    let token = state.gh_token().await.ok().flatten().map(|t| t.access_token);
    blocking(move || branches::update(Path::new(&path), &name, token)).await
}

#[tauri::command]
pub async fn git_delete_branch(path: String, name: String) -> R<()> {
    blocking(move || branches::delete(Path::new(&path), &name)).await
}

/// Put a repo's detached submodules on their branch (`only` = one submodule's folder).
#[tauri::command]
pub async fn attach_submodules(path: String, only: Option<String>) -> R<Vec<git::AttachOutcome>> {
    blocking(move || git::attach_submodules(Path::new(&path), only.as_deref().map(Path::new))).await
}

#[tauri::command]
pub async fn app_icons() -> R<system::AppIcons> {
    blocking(|| Ok(system::app_icons())).await
}

// ---------- categories ----------

#[tauri::command]
pub fn add_category(root: String, name: String, color: String) -> R<WorkspaceConfig> {
    workspace::add_category(Path::new(&root), &name, &color)
}

#[tauri::command]
pub fn update_category(root: String, id: String, name: String, color: String) -> R<WorkspaceConfig> {
    workspace::update_category(Path::new(&root), &id, &name, &color)
}

#[tauri::command]
pub async fn delete_category(root: String, id: String) -> R<WorkspaceConfig> {
    blocking(move || workspace::delete_category(Path::new(&root), &id)).await
}

#[tauri::command]
pub fn set_project_categories(root: String, path: String, categories: Vec<String>) -> R<ProjectEntry> {
    workspace::set_categories(Path::new(&root), Path::new(&path), categories)
}

#[tauri::command]
pub fn set_project_name(root: String, path: String, name: Option<String>) -> R<ProjectEntry> {
    workspace::set_name(Path::new(&root), Path::new(&path), name.as_deref())
}

// ---------- moving, archiving, adopting ----------

#[tauri::command]
pub async fn move_project(root: String, path: String, target: Section) -> R<ProjectEntry> {
    blocking(move || workspace::move_project(Path::new(&root), Path::new(&path), target)).await
}

#[tauri::command]
pub async fn unarchive_project(root: String, path: String) -> R<ProjectEntry> {
    blocking(move || workspace::unarchive(Path::new(&root), Path::new(&path))).await
}

#[tauri::command]
pub fn adopt_project(root: String, path: String) -> R<ProjectEntry> {
    workspace::adopt(Path::new(&root), Path::new(&path))
}

// ---------- new project ----------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewProjectArgs {
    name: String,
    section: Section,
    init_git: bool,
    categories: Vec<String>,
}

#[tauri::command]
pub fn suggest_folder_name(name: String) -> String {
    naming::sanitize_component(name.trim())
}

#[tauri::command]
pub fn create_project(root: String, args: NewProjectArgs) -> R<ProjectEntry> {
    let root = PathBuf::from(root);
    workspace::require_workspace(&root)?;
    not_archive(args.section)?;
    if args.name.trim().is_empty() {
        return Err(msg("Give the project a name."));
    }
    let folder = naming::sanitize_component(args.name.trim());
    workspace::ensure_name_free(&root, &folder)?;
    let dest = workspace::section_dir(&root, args.section).join(&folder);
    std::fs::create_dir_all(&dest).map_err(|e| crate::error::io_ctx(e, "create", &dest))?;
    if args.init_git {
        git::init_repo(&dest)?;
    }
    let mut m = ProjectMeta::new(
        args.section,
        Source { kind: SourceKind::New, remote: None, imported_from: None },
        args.categories,
    );
    m.name = clean_name(Some(&args.name));
    workspace::save_project_meta(&root, &dest, &m)?;
    workspace::project_entry(&root, &dest)
}

// ---------- clone ----------

#[tauri::command]
pub fn parse_repo(input: String) -> Option<RepoRef> {
    naming::parse_repo_input(&input)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloneArgs {
    input: String,
    section: Section,
    categories: Vec<String>,
    recurse_submodules: bool,
    folder: Option<String>,
    /// Display name; unset = show owner/repo.
    display_name: Option<String>,
    /// Branch to check out; unset = the remote's default branch.
    branch: Option<String>,
    /// Only the latest commit of that one branch.
    #[serde(default)]
    shallow: bool,
}

#[tauri::command]
pub async fn clone_repo(
    app: AppHandle,
    state: State<'_, AppState>,
    root: String,
    job_id: String,
    args: CloneArgs,
) -> R<ProjectEntry> {
    let root = PathBuf::from(root);
    workspace::require_workspace(&root)?;
    not_archive(args.section)?;
    let mut r = naming::parse_repo_input(&args.input)
        .ok_or_else(|| msg("That doesn't look like a repo. Use owner/repo or a GitHub URL."))?;

    // A failed renewal just means cloning without a sign-in; public repos still work.
    let token = if r.is_github() { state.gh_token().await.ok().flatten().map(|t| t.access_token) } else { None };
    if r.is_github() {
        // Get GitHub's canonical owner/name casing; fall back to what was typed if offline.
        match github::get_repo(&state.http, token.as_deref(), &r.owner, &r.repo).await {
            Ok(Some(g)) => r = RepoRef::new("github.com", &g.owner, &g.name),
            Ok(None) => {
                return Err(msg(if token.is_some() {
                    format!("{}/{} wasn't found, or your account can't see it.", r.owner, r.repo)
                } else {
                    format!("{}/{} wasn't found. If it's private, sign in to GitHub in Settings first.", r.owner, r.repo)
                }))
            }
            Err(_) => {}
        }
    }

    let folder = match args.folder.as_deref().map(str::trim).filter(|f| !f.is_empty()) {
        Some(f) => naming::sanitize_component(f),
        None => r.folder_name(),
    };
    workspace::ensure_name_free(&root, &folder)?;
    let dest = workspace::section_dir(&root, args.section).join(&folder);

    let cancel = state.start_job(&job_id);
    let on = emitter(&app, &job_id);
    let url = r.clone_url();
    let dest2 = dest.clone();
    let recurse = args.recurse_submodules;
    let branch = args.branch.clone();
    let shallow = args.shallow;
    let result = blocking(move || git::clone_repo(&url, &dest2, token, recurse, branch.as_deref(), shallow, cancel, on)).await;
    state.end_job(&job_id);
    result?;

    let remote: RemoteInfo = (&r).into();
    let display = clean_name(args.display_name.as_deref());
    let m = match workspace::find_previous(&root, &r.key()) {
        Some(mut prev) => {
            prev.section = args.section;
            prev.archived_from = None;
            prev.archived_at = None;
            if !args.categories.is_empty() {
                prev.categories = args.categories;
            }
            if display.is_some() {
                prev.name = display;
            }
            prev.source.remote = Some(remote);
            prev
        }
        None => {
            let mut m = ProjectMeta::new(
                args.section,
                Source { kind: SourceKind::Clone, remote: Some(remote), imported_from: None },
                args.categories,
            );
            m.name = display;
            m
        }
    };
    workspace::save_project_meta(&root, &dest, &m)?;
    workspace::project_entry(&root, &dest)
}

#[tauri::command]
pub fn cancel_job(state: State<'_, AppState>, job_id: String) {
    if let Some(f) = state.jobs.lock().unwrap().get(&job_id) {
        f.store(true, Ordering::Relaxed);
    }
}

// ---------- import ----------

#[tauri::command]
pub fn inspect_import(path: String) -> R<ImportInspection> {
    importer::inspect(Path::new(&path))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportArgs {
    source: String,
    section: Section,
    /// Folder name in the workspace.
    name: String,
    /// Display name; unset = show owner/repo (or the folder name).
    display_name: Option<String>,
    categories: Vec<String>,
    skip_regenerable: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    project: ProjectEntry,
    report: ImportReport,
}

#[tauri::command]
pub async fn import_project(
    app: AppHandle,
    state: State<'_, AppState>,
    root: String,
    job_id: String,
    args: ImportArgs,
) -> R<ImportResult> {
    let root = PathBuf::from(root);
    workspace::require_workspace(&root)?;
    not_archive(args.section)?;
    let src = PathBuf::from(&args.source);
    if let (Ok(cs), Ok(cr)) = (std::fs::canonicalize(&src), std::fs::canonicalize(&root)) {
        if cs.starts_with(&cr) {
            return Err(msg("That folder is already inside this workspace. Use Adopt instead."));
        }
    }
    let folder = naming::sanitize_component(args.name.trim());
    if folder == "_" {
        return Err(msg("Give the project a name."));
    }
    workspace::ensure_name_free(&root, &folder)?;
    let dest = workspace::section_dir(&root, args.section).join(&folder);

    let cancel = state.start_job(&job_id);
    let on = emitter(&app, &job_id);
    let (src2, dest2, skip) = (src.clone(), dest.clone(), args.skip_regenerable);
    let result = blocking(move || importer::copy_project(&src2, &dest2, skip, cancel, on)).await;
    state.end_job(&job_id);
    let report = result?;

    let remote = crate::meta::detect_remote(&dest);
    let prev = remote.as_ref().and_then(|r| workspace::find_previous(&root, &r.key()));
    let source = Source {
        kind: SourceKind::Import,
        remote: remote.as_ref().map(Into::into),
        imported_from: Some(src.to_string_lossy().to_string()),
    };
    let display = clean_name(args.display_name.as_deref());
    let m = match prev {
        Some(mut p) => {
            p.section = args.section;
            p.archived_from = None;
            p.archived_at = None;
            if !args.categories.is_empty() {
                p.categories = args.categories;
            }
            if display.is_some() {
                p.name = display;
            }
            p.source = source;
            p
        }
        None => {
            let mut m = ProjectMeta::new(args.section, source, args.categories);
            m.name = display;
            m
        }
    };
    workspace::save_project_meta(&root, &dest, &m)?;
    Ok(ImportResult { project: workspace::project_entry(&root, &dest)?, report })
}

// ---------- GitHub ----------

#[tauri::command]
pub async fn github_account(state: State<'_, AppState>) -> R<Option<Account>> {
    match state.gh_token().await? {
        None => Ok(None),
        Some(t) => Ok(Some(github::account(&state.http, &t).await?)),
    }
}

fn client_id(app: &AppHandle) -> AppResult<String> {
    settings::load(app)
        .github_client_id
        .ok_or_else(|| msg("Add your GitHub OAuth App's Client ID first (Settings → GitHub)."))
}

#[tauri::command]
pub async fn github_start_device(app: AppHandle, state: State<'_, AppState>) -> R<DeviceCode> {
    let id = client_id(&app)?;
    github::start_device(&state.http, &id).await
}

#[tauri::command]
pub async fn github_poll_device(app: AppHandle, state: State<'_, AppState>, device_code: String) -> R<PollResult> {
    let id = client_id(&app)?;
    github::poll_device(&state.http, &id, &device_code).await
}

#[tauri::command]
pub async fn github_sign_in_token(state: State<'_, AppState>, token: String) -> R<Account> {
    github::sign_in_with_token(&state.http, &token).await
}

#[tauri::command]
pub fn github_sign_out() -> R<()> {
    github::clear_token()
}

#[tauri::command]
pub async fn github_search(state: State<'_, AppState>, query: String) -> R<Vec<GhRepo>> {
    let token = state.gh_token().await.ok().flatten().map(|t| t.access_token);
    github::search_repos(&state.http, token.as_deref(), &query).await
}

#[tauri::command]
pub async fn github_branches(state: State<'_, AppState>, owner: String, repo: String) -> R<Vec<String>> {
    let token = state.gh_token().await.ok().flatten().map(|t| t.access_token);
    github::list_branches(&state.http, token.as_deref(), &owner, &repo).await
}

#[tauri::command]
pub async fn github_list_repos(state: State<'_, AppState>) -> R<Vec<GhRepo>> {
    let t = state.gh_token().await?.ok_or_else(|| msg("Sign in to GitHub first."))?;
    github::list_repos(&state.http, &t.access_token).await
}

// ---------- open in other apps ----------

#[tauri::command]
pub fn open_in_explorer(path: String) -> R<()> {
    system::open_in_explorer(Path::new(&path))
}

#[tauri::command]
pub fn open_in_vscode(path: String) -> R<()> {
    system::open_in_vscode(Path::new(&path))
}

#[tauri::command]
pub fn open_in_terminal(path: String) -> R<()> {
    system::open_in_terminal(Path::new(&path))
}

#[tauri::command]
pub fn open_url(app: AppHandle, url: String) -> R<()> {
    if !url.starts_with("https://") {
        return Err(msg("Only https links can be opened."));
    }
    app.opener().open_url(url, None::<&str>).map_err(|e| msg(e.to_string()))
}
