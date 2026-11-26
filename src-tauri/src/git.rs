//! Git operations through the bundled libgit2. No system git is used.

use crate::error::{msg, AppResult};
use chrono::{DateTime, TimeZone, Utc};
use git2::build::{CheckoutBuilder, RepoBuilder};
use git2::{
    BranchType, Cred, CredentialType, FetchOptions, ProxyOptions, RemoteCallbacks, Repository, Status,
    StatusOptions, SubmoduleUpdateOptions,
};
use serde::Serialize;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const UNPUSHED_CAP: u32 = 10_000;
const THROTTLE: Duration = Duration::from_millis(120);

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CommitInfo {
    pub id: String,
    pub summary: String,
    pub author: String,
    pub time: Option<DateTime<Utc>>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RemoteEntry {
    pub name: String,
    pub url: String,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct GitStatus {
    pub branch: Option<String>,
    pub detached: bool,
    pub unborn: bool,
    pub staged: u32,
    pub modified: u32,
    pub untracked: u32,
    pub conflicted: u32,
    pub upstream: Option<String>,
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
    /// Commits reachable from HEAD that are on no remote-tracking branch.
    pub unpushed: u32,
    pub unpushed_capped: bool,
    pub remotes: Vec<RemoteEntry>,
    pub last_commit: Option<CommitInfo>,
    pub submodules: u32,
    pub worktrees: u32,
    /// The remote's default branch (from `origin/HEAD`), e.g. "main".
    pub default_branch: Option<String>,
    /// Cloned with "latest commit only": history is cut off.
    pub shallow: bool,
    /// `origin` only fetches one branch.
    pub single_branch: bool,
}

/// Let libgit2 handle paths longer than Windows' 260-character limit
/// (`core.longpaths`) in every repo it opens or clones, submodules included.
/// It's set in a config file of Hangar's own, loaded as libgit2's "XDG"
/// level, so the user's own git config is never touched.
pub fn enable_long_paths(dir: &Path) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let cfg = dir.join("git");
        if std::fs::create_dir_all(&cfg).is_ok() && std::fs::write(cfg.join("config"), "[core]\n\tlongpaths = true\n").is_ok() {
            // SAFETY: runs once, at startup, before any other git work.
            unsafe {
                let _ = git2::opts::set_search_path(git2::ConfigLevel::XDG, cfg.to_string_lossy().as_ref());
            }
        }
    });
}

pub(crate) fn commit_info(repo: &Repository, oid: Option<git2::Oid>) -> Option<CommitInfo> {
    let oid = oid?;
    let c = repo.find_commit(oid).ok()?;
    Some(CommitInfo {
        id: oid.to_string()[..7].to_string(),
        summary: c.summary().ok().flatten().unwrap_or("").to_string(),
        author: c.author().name().unwrap_or("").to_string(),
        time: Utc.timestamp_opt(c.time().seconds(), 0).single(),
    })
}

/// Remove `user:password@` from a URL before showing it anywhere.
pub fn redact_url(url: &str) -> String {
    if let Some((scheme, rest)) = url.split_once("://") {
        if let Some((auth, host_path)) = rest.split_once('@') {
            if !auth.contains('/') {
                return format!("{scheme}://{host_path}");
            }
        }
    }
    url.to_string()
}

pub fn status(path: &Path) -> AppResult<GitStatus> {
    let repo = Repository::open(path)?;
    let mut st = GitStatus::default();

    let head = match repo.head() {
        Ok(h) => Some(h),
        Err(e) if e.code() == git2::ErrorCode::UnbornBranch => None,
        Err(e) => return Err(e.into()),
    };
    let head_oid = head.as_ref().and_then(|h| h.target());
    match &head {
        Some(h) if h.is_branch() => st.branch = h.shorthand().ok().map(String::from),
        Some(_) => st.detached = true,
        None => {
            st.unborn = true;
            if let Ok(r) = repo.find_reference("HEAD") {
                st.branch = r
                    .symbolic_target()
                    .ok()
                    .flatten()
                    .and_then(|t| t.strip_prefix("refs/heads/"))
                    .map(String::from);
            }
        }
    }

    let mut opts = StatusOptions::new();
    opts.include_untracked(true).recurse_untracked_dirs(false).exclude_submodules(true).include_ignored(false);
    for e in repo.statuses(Some(&mut opts))?.iter() {
        let s = e.status();
        if s.is_conflicted() {
            st.conflicted += 1;
            continue;
        }
        if s.intersects(
            Status::INDEX_NEW | Status::INDEX_MODIFIED | Status::INDEX_DELETED | Status::INDEX_RENAMED | Status::INDEX_TYPECHANGE,
        ) {
            st.staged += 1;
        }
        if s.intersects(Status::WT_MODIFIED | Status::WT_DELETED | Status::WT_RENAMED | Status::WT_TYPECHANGE) {
            st.modified += 1;
        }
        if s.contains(Status::WT_NEW) {
            st.untracked += 1;
        }
    }

    if let (Some(name), Some(local)) = (st.branch.clone(), head_oid) {
        if let Ok(up) = repo.find_branch(&name, BranchType::Local).and_then(|b| b.upstream()) {
            st.upstream = up.name().ok().flatten().map(String::from);
            if let Some(up_oid) = up.get().target() {
                if let Ok((a, b)) = repo.graph_ahead_behind(local, up_oid) {
                    st.ahead = Some(a as u32);
                    st.behind = Some(b as u32);
                }
            }
        }
    }

    if let Some(oid) = head_oid {
        let mut walk = repo.revwalk()?;
        walk.push(oid)?;
        for r in repo.references()?.flatten() {
            if r.name().is_ok_and(|n| n.starts_with("refs/remotes/")) {
                if let Some(o) = r.target() {
                    let _ = walk.hide(o);
                }
            }
        }
        for c in walk {
            if c.is_err() {
                break;
            }
            st.unpushed += 1;
            if st.unpushed >= UNPUSHED_CAP {
                st.unpushed_capped = true;
                break;
            }
        }
        st.last_commit = commit_info(&repo, Some(oid));
    }
    st.default_branch = default_remote_branch(&repo);
    st.shallow = repo.is_shallow();
    st.single_branch = repo
        .find_remote("origin")
        .ok()
        .and_then(|r| r.fetch_refspecs().ok().map(|a| a.iter().flatten().flatten().map(String::from).collect::<Vec<_>>()))
        .is_some_and(|specs| !specs.is_empty() && specs.iter().all(|s| !s.contains('*')));

    if let Ok(names) = repo.remotes() {
        for n in names.iter().flatten().flatten() {
            if let Ok(r) = repo.find_remote(n) {
                st.remotes.push(RemoteEntry { name: n.to_string(), url: redact_url(r.url().unwrap_or("")) });
            }
        }
    }
    st.submodules = repo.submodules().map(|v| v.len() as u32).unwrap_or(0);
    st.worktrees = repo.worktrees().map(|w| w.len() as u32).unwrap_or(0);
    Ok(st)
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct JobProgress {
    pub stage: String,
    pub current: u64,
    pub total: u64,
    pub bytes: u64,
    pub detail: Option<String>,
}

pub type ProgressFn = Arc<dyn Fn(JobProgress) + Send + Sync>;

fn callbacks<'a>(token: Option<String>, cancel: Arc<AtomicBool>, on: ProgressFn, label: Option<String>) -> RemoteCallbacks<'a> {
    let mut cb = RemoteCallbacks::new();
    let mut tried = false;
    cb.credentials(move |url, _user, allowed| {
        if tried {
            return Err(git2::Error::from_str(
                "GitHub rejected the sign-in, or the repo doesn't exist or isn't shared with your account.",
            ));
        }
        tried = true;
        // The GitHub token is only ever sent to github.com.
        let to_github = url.starts_with("https://github.com/");
        match &token {
            Some(t) if to_github && allowed.contains(CredentialType::USER_PASS_PLAINTEXT) => {
                Cred::userpass_plaintext("x-access-token", t)
            }
            _ => Err(git2::Error::from_str("This repo needs a login. Sign in to GitHub in Hangar's settings.")),
        }
    });
    let mut last = Instant::now() - THROTTLE;
    cb.transfer_progress(move |s| {
        if cancel.load(Ordering::Relaxed) {
            return false;
        }
        let receiving = s.received_objects() < s.total_objects();
        let finished = !receiving && s.indexed_deltas() == s.total_deltas();
        if finished || last.elapsed() >= THROTTLE {
            last = Instant::now();
            let (stage, cur, tot) = if receiving || s.total_deltas() == 0 {
                ("receiving", s.received_objects(), s.total_objects())
            } else {
                ("resolving", s.indexed_deltas(), s.total_deltas())
            };
            on(JobProgress {
                stage: stage.into(),
                current: cur as u64,
                total: tot as u64,
                bytes: s.received_bytes() as u64,
                detail: label.clone(),
            });
        }
        true
    });
    cb
}

fn fetch_options<'a>(token: &Option<String>, cancel: &Arc<AtomicBool>, on: &ProgressFn, label: Option<String>) -> FetchOptions<'a> {
    let mut fo = FetchOptions::new();
    fo.remote_callbacks(callbacks(token.clone(), cancel.clone(), on.clone(), label));
    let mut po = ProxyOptions::new();
    po.auto();
    fo.proxy_options(po);
    fo
}

/// Delete a folder even when git made some files read-only (normal on Windows).
pub fn remove_dir_all_force(p: &Path) -> std::io::Result<()> {
    if !p.exists() {
        return Ok(());
    }
    if std::fs::remove_dir_all(p).is_ok() {
        return Ok(());
    }
    for e in walkdir::WalkDir::new(p).into_iter().flatten() {
        if let Ok(md) = e.metadata() {
            let mut perm = md.permissions();
            if perm.readonly() {
                #[allow(clippy::permissions_set_readonly_false)]
                perm.set_readonly(false);
                let _ = std::fs::set_permissions(e.path(), perm);
            }
        }
    }
    std::fs::remove_dir_all(p)
}

/// Clone `url` into `dest` (which must not exist), checking out `branch` (or
/// the remote's default). `shallow` downloads only the latest commit of that
/// one branch. On failure or cancel the partly cloned folder is removed.
#[allow(clippy::too_many_arguments)]
pub fn clone_repo(
    url: &str,
    dest: &Path,
    token: Option<String>,
    recurse_submodules: bool,
    branch: Option<&str>,
    shallow: bool,
    cancel: Arc<AtomicBool>,
    on: ProgressFn,
) -> AppResult<()> {
    if dest.exists() {
        return Err(msg(format!("{} already exists.", dest.display())));
    }
    let branch = branch.map(str::trim).filter(|b| !b.is_empty());
    let result = (|| -> AppResult<()> {
        let mut fo = fetch_options(&token, &cancel, &on, None);
        // Latest commit only: one branch (asking the remote for its default if
        // none was picked), and a depth-1 fetch.
        let single = if shallow {
            fo.depth(1);
            on(JobProgress { stage: "connecting".into(), current: 0, total: 0, bytes: 0, detail: None });
            Some(match branch {
                Some(b) => b.to_string(),
                None => remote_default_branch(url, &token)?,
            })
        } else {
            None
        };
        let on_co = on.clone();
        let mut last = Instant::now() - THROTTLE;
        let mut co = CheckoutBuilder::new();
        co.progress(move |_p, cur, total| {
            if cur == total || last.elapsed() >= THROTTLE {
                last = Instant::now();
                on_co(JobProgress { stage: "checkout".into(), current: cur as u64, total: total as u64, bytes: 0, detail: None });
            }
        });
        let mut builder = RepoBuilder::new();
        builder.fetch_options(fo).with_checkout(co);
        if let Some(b) = single.as_deref().or(branch) {
            builder.branch(b);
        }
        if let Some(b) = single.clone() {
            // `origin` tracks only this branch, so later fetches stay small too.
            builder.remote_create(move |repo, name, url| {
                repo.remote_with_fetch(name, url, &format!("+refs/heads/{b}:refs/remotes/{name}/{b}"))
            });
        }
        let wanted = single.as_deref().or(branch);
        let repo = builder.clone(url, dest).map_err(|e| match wanted {
            Some(b) if e.code() == git2::ErrorCode::NotFound => msg(format!("The remote has no branch called {b}.")),
            _ => e.into(),
        })?;
        if recurse_submodules {
            update_submodules(&repo, &token, &cancel, &on, 0)?;
        }
        Ok(())
    })();
    if let Err(e) = result {
        let _ = remove_dir_all_force(dest);
        if cancel.load(Ordering::Relaxed) {
            return Err(msg("Cancelled."));
        }
        return Err(e);
    }
    Ok(())
}

fn update_submodules(repo: &Repository, token: &Option<String>, cancel: &Arc<AtomicBool>, on: &ProgressFn, depth: u32) -> AppResult<()> {
    if depth > 8 {
        return Ok(());
    }
    let names: Vec<String> = repo.submodules()?.iter().filter_map(|s| s.name().ok().map(String::from)).collect();
    for name in names {
        if cancel.load(Ordering::Relaxed) {
            return Err(msg("Cancelled."));
        }
        on(JobProgress { stage: "submodule".into(), current: 0, total: 0, bytes: 0, detail: Some(name.clone()) });
        repo.find_submodule(&name)?.init(false)?;

        // The bundled git has no SSH support, so point SSH-style submodule URLs
        // at HTTPS. This only changes the local .git/config, never .gitmodules.
        let key = format!("submodule.{name}.url");
        let mut cfg = repo.config()?;
        if let Ok(url) = cfg.get_string(&key) {
            if !url.starts_with("https://") && !url.starts_with("http://") {
                if let Some(r) = crate::naming::parse_repo_input(&url) {
                    cfg.set_str(&key, &r.clone_url())?;
                }
            }
        }

        let mut sm = repo.find_submodule(&name)?;
        let mut opts = SubmoduleUpdateOptions::new();
        opts.fetch(fetch_options(token, cancel, on, Some(name.clone())));
        sm.update(false, Some(&mut opts)).map_err(|e| msg(format!("Submodule {name}: {}", e.message())))?;
        if let Ok(sub) = sm.open() {
            update_submodules(&sub, token, cancel, on, depth + 1)?;
            // Best effort: leave the submodule on its branch instead of a detached HEAD.
            if let Some(dir) = sub.workdir() {
                let _ = attach_branch(dir, preferred_branch(repo, &sm).as_deref());
            }
        }
    }
    Ok(())
}

/// The branch a submodule should be on: `branch = …` in .gitmodules, where `.`
/// means "the same branch as the parent".
fn preferred_branch(parent: &Repository, sm: &git2::Submodule) -> Option<String> {
    match sm.branch().ok().flatten() {
        Some(".") => parent.head().ok().and_then(|h| h.shorthand().ok().map(String::from)),
        Some(b) if !b.is_empty() => Some(b.to_string()),
        _ => None,
    }
}

pub(crate) fn default_remote_branch(repo: &Repository) -> Option<String> {
    if let Ok(r) = repo.find_reference("refs/remotes/origin/HEAD") {
        if let Some(b) = r.symbolic_target().ok().flatten().and_then(|t| t.strip_prefix("refs/remotes/origin/")) {
            return Some(b.to_string());
        }
    }
    ["main", "master"]
        .into_iter()
        .find(|b| repo.find_reference(&format!("refs/remotes/origin/{b}")).is_ok())
        .map(String::from)
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AttachOutcome {
    /// Path of the repo inside its parent (for submodules), or its folder.
    pub path: String,
    /// The branch it is on now; `None` = still detached.
    pub branch: Option<String>,
    pub note: Option<String>,
}

/// Put a detached repo (typically a submodule) on its branch without changing
/// any files. Only done when the checked-out commit is on that remote branch;
/// a local branch with commits of its own is never moved.
pub fn attach_branch(path: &Path, preferred: Option<&str>) -> AppResult<AttachOutcome> {
    let repo = Repository::open(path)?;
    let label = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let done = |branch: Option<String>, note: Option<String>| Ok(AttachOutcome { path: label.clone(), branch, note });
    if !repo.head_detached()? {
        return done(repo.head().ok().and_then(|h| h.shorthand().ok().map(String::from)), None);
    }
    let head = repo.head()?.target().ok_or_else(|| msg("HEAD doesn't point at a commit."))?;
    let Some(branch) = preferred.map(String::from).or_else(|| default_remote_branch(&repo)) else {
        return done(None, Some("No remote branch to attach to.".into()));
    };
    let Ok(tip) = repo.refname_to_id(&format!("refs/remotes/origin/{branch}")) else {
        return done(None, Some(format!("origin/{branch} doesn't exist.")));
    };
    if tip != head && !repo.graph_descendant_of(tip, head)? {
        return done(None, Some(format!("The pinned commit isn't on origin/{branch}, so it stays detached.")));
    }
    let local = format!("refs/heads/{branch}");
    match repo.refname_to_id(&local) {
        Ok(id) if id == head => {}
        // Everything on the local branch is also on the remote: safe to point it at the pinned commit.
        Ok(id) if id == tip || repo.graph_descendant_of(tip, id)? => {
            repo.reference(&local, head, true, "hangar: attach to branch")?;
        }
        Ok(_) => return done(None, Some(format!("Local {branch} has commits of its own, so it was left alone."))),
        Err(_) => {
            repo.branch(&branch, &repo.find_commit(head)?, false)?;
        }
    }
    if let Ok(mut b) = repo.find_branch(&branch, BranchType::Local) {
        let _ = b.set_upstream(Some(&format!("origin/{branch}")));
    }
    repo.set_head(&local)?;
    let behind = if tip != head { Some(format!("Behind origin/{branch}: the parent pins an older commit.")) } else { None };
    done(Some(branch), behind)
}

/// Attach every checked-out submodule (or just the one at `only`) to its branch.
pub fn attach_submodules(path: &Path, only: Option<&Path>) -> AppResult<Vec<AttachOutcome>> {
    let repo = Repository::open(path)?;
    let Some(workdir) = repo.workdir() else { return Ok(Vec::new()) };
    let mut out = Vec::new();
    for sm in repo.submodules()? {
        let dir = workdir.join(sm.path());
        if only.is_some_and(|o| o != dir) || !dir.join(".git").exists() {
            continue;
        }
        let mut r = attach_branch(&dir, preferred_branch(&repo, &sm).as_deref())?;
        r.path = sm.path().to_string_lossy().replace('\\', "/");
        out.push(r);
    }
    Ok(out)
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct SubmoduleInfo {
    pub name: String,
    /// Path inside the parent repo, with forward slashes (e.g. `apps/api`).
    pub path: String,
    pub abs_path: String,
    pub url: Option<String>,
    pub remote: Option<crate::naming::RepoRef>,
    /// The submodule's files are checked out.
    pub initialized: bool,
    /// Commit the parent repo points at, and the one actually checked out.
    pub recorded: Option<String>,
    pub checked_out: Option<String>,
    /// Checked-out commit differs from the one the parent records.
    pub moved: bool,
    pub status: Option<GitStatus>,
}

fn short(oid: Option<git2::Oid>) -> Option<String> {
    oid.map(|o| o.to_string()[..7].to_string())
}

/// Resolve a submodule URL, including relative ones like `../api.git`,
/// which git interprets relative to the parent's remote.
pub fn resolve_submodule_url(url: &str, parent: Option<&crate::naming::RepoRef>) -> Option<crate::naming::RepoRef> {
    if !(url.starts_with("../") || url.starts_with("./")) {
        return crate::naming::parse_repo_input(url);
    }
    let p = parent?;
    let mut segs = vec![p.owner.clone(), p.repo.clone()];
    let mut rest = url;
    loop {
        if let Some(r) = rest.strip_prefix("../") {
            segs.pop();
            rest = r;
        } else if let Some(r) = rest.strip_prefix("./") {
            rest = r;
        } else {
            break;
        }
    }
    segs.push(rest.to_string());
    crate::naming::parse_repo_input(&format!("{}/{}", p.host, segs.join("/")))
}

/// One level of submodules with their own git status.
pub fn submodules(path: &Path) -> AppResult<Vec<SubmoduleInfo>> {
    let repo = Repository::open(path)?;
    let Some(workdir) = repo.workdir() else { return Ok(Vec::new()) };
    let parent_remote = crate::meta::detect_remote(path);
    let mut out = Vec::new();
    for sm in repo.submodules()? {
        let rel = sm.path().to_path_buf();
        let abs = workdir.join(&rel);
        let url = sm.url().ok().flatten().map(redact_url);
        let remote = url.as_deref().and_then(|u| resolve_submodule_url(u, parent_remote.as_ref()));
        let initialized = abs.join(".git").exists();
        let status = if initialized { status(&abs).ok() } else { None };
        let (recorded, checked_out) = (sm.head_id(), sm.workdir_id());
        out.push(SubmoduleInfo {
            name: sm.name().unwrap_or("").to_string(),
            path: rel.to_string_lossy().replace('\\', "/"),
            abs_path: abs.to_string_lossy().to_string(),
            url,
            remote,
            initialized,
            moved: matches!((recorded, checked_out), (Some(a), Some(b)) if a != b),
            recorded: short(recorded),
            checked_out: short(checked_out),
            status,
        });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

/* ---------- fetch & pull ---------- */

/// The bundled git has no SSH, so SSH remotes on known hosts are fetched over HTTPS.
fn https_for(url: &str) -> Option<String> {
    if url.starts_with("https://") || url.starts_with("http://") || url.starts_with("file://") {
        return None;
    }
    crate::naming::parse_repo_input(url).map(|r| r.clone_url())
}

/// Fetch one remote, updating `refs/remotes/<name>/*`.
pub(crate) fn fetch_remote(repo: &Repository, name: &str, token: &Option<String>) -> AppResult<usize> {
    fetch_remote_with(repo, name, token, None)
}

/// Fetch one remote using its own configured refspecs, so a single-branch
/// clone stays single-branch. `depth` = shallow depth (`UNSHALLOW` for all history).
fn fetch_remote_with(repo: &Repository, name: &str, token: &Option<String>, depth: Option<i32>) -> AppResult<usize> {
    let cancel = Arc::new(AtomicBool::new(false));
    let on: ProgressFn = Arc::new(|_| {});
    let mut fo = fetch_options(token, &cancel, &on, None);
    if let Some(d) = depth {
        fo.depth(d);
    }
    let (url, specs) = {
        let r = repo.find_remote(name)?;
        let specs: Vec<String> = r.fetch_refspecs()?.iter().flatten().flatten().map(String::from).collect();
        (r.url().map(String::from).unwrap_or_default(), specs)
    };
    let specs = if specs.is_empty() { vec![format!("+refs/heads/*:refs/remotes/{name}/*")] } else { specs };
    let mut remote = match https_for(&url) {
        Some(https) => repo.remote_anonymous(&https)?,
        None => repo.find_remote(name)?,
    };
    let refs: Vec<&str> = specs.iter().map(String::as_str).collect();
    remote
        .fetch(&refs, Some(&mut fo), Some(&format!("hangar: fetch {name}")))
        .map_err(|e| msg(format!("Fetching {name}: {}", e.message())))?;
    Ok(remote.stats().received_objects())
}

/// libgit2's "fetch all history" depth (GIT_FETCH_DEPTH_UNSHALLOW).
const UNSHALLOW: i32 = i32::MAX;

/// The remote's default branch, asked over the network (e.g. "main").
fn remote_default_branch(url: &str, token: &Option<String>) -> AppResult<String> {
    let mut remote = git2::Remote::create_detached(url)?;
    let cb = callbacks(token.clone(), Arc::new(AtomicBool::new(false)), Arc::new(|_| {}), None);
    let mut po = ProxyOptions::new();
    po.auto();
    let conn = remote.connect_auth(git2::Direction::Fetch, Some(cb), Some(po))?;
    let buf = conn.default_branch()?;
    let name = buf.as_str().map_err(|_| msg("The remote's default branch name isn't valid text."))?;
    Ok(name.strip_prefix("refs/heads/").unwrap_or(name).to_string())
}

/// Turn a "latest commit only" clone into a full one: every branch, all history.
pub fn full_history(path: &Path, token: Option<String>) -> AppResult<()> {
    let repo = Repository::open(path)?;
    let name = repo
        .remotes()?
        .iter()
        .flatten()
        .flatten()
        .map(String::from)
        .find(|n| n == "origin")
        .or_else(|| repo.remotes().ok()?.iter().flatten().flatten().next().map(String::from))
        .ok_or_else(|| msg("This repo has no remote."))?;
    let wide = format!("+refs/heads/*:refs/remotes/{name}/*");
    let has_wide = repo.find_remote(&name)?.fetch_refspecs()?.iter().flatten().flatten().any(|s| s == wide);
    if !has_wide {
        repo.remote_add_fetch(&name, &wide)?;
    }
    fetch_remote_with(&repo, &name, &token, if repo.is_shallow() { Some(UNSHALLOW) } else { None })?;
    Ok(())
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct FetchOutcome {
    pub remotes: Vec<String>,
    pub received_objects: usize,
}

/// Fetch every remote of the repo. Files and branches are not touched.
pub fn fetch_all(path: &Path, token: Option<String>) -> AppResult<FetchOutcome> {
    let repo = Repository::open(path)?;
    let names: Vec<String> = repo.remotes()?.iter().flatten().flatten().map(String::from).collect();
    if names.is_empty() {
        return Err(msg("This repo has no remote to fetch from."));
    }
    let mut out = FetchOutcome { remotes: Vec::new(), received_objects: 0 };
    for name in names {
        out.received_objects += fetch_remote(&repo, &name, &token)?;
        out.remotes.push(name);
    }
    Ok(out)
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PullOutcome {
    pub branch: String,
    pub upstream: String,
    /// Commits fast-forwarded; 0 = already up to date.
    pub commits: usize,
    /// Submodules whose pinned commit changed with this pull.
    pub moved_submodules: Vec<String>,
}

/// Fetch the upstream, then fast-forward the current branch to it.
/// Never merges or rebases, and never overwrites local changes.
pub fn pull_ff(path: &Path, token: Option<String>) -> AppResult<PullOutcome> {
    let repo = Repository::open(path)?;
    let head = repo.head().map_err(|_| msg("This repo has no commits yet."))?;
    if !head.is_branch() {
        return Err(msg("Detached HEAD: put the repo on a branch before pulling."));
    }
    let branch = head.shorthand()?.to_string();
    let refname = head.name()?.to_string();
    let local = head.target().ok_or_else(|| msg("HEAD doesn't point at a commit."))?;
    drop(head);

    let upstream_name = repo
        .find_branch(&branch, BranchType::Local)?
        .upstream()
        .map_err(|_| msg(format!("{branch} has no upstream branch to pull from.")))?
        .name()?
        .unwrap_or_default()
        .to_string();
    let remote = repo.branch_upstream_remote(&refname)?;
    let remote = remote.as_str().map_err(|_| msg("The upstream remote's name isn't valid text."))?.to_string();
    fetch_remote(&repo, &remote, &token)?;

    let target = repo
        .find_branch(&branch, BranchType::Local)?
        .upstream()?
        .get()
        .target()
        .ok_or_else(|| msg(format!("{upstream_name} doesn't point at a commit.")))?;
    let (analysis, _) = repo.merge_analysis(&[&repo.find_annotated_commit(target)?])?;
    let done = |commits| PullOutcome { branch: branch.clone(), upstream: upstream_name.clone(), commits, moved_submodules: Vec::new() };
    if analysis.is_up_to_date() {
        return Ok(done(0));
    }
    let (ahead, behind) = repo.graph_ahead_behind(local, target)?;
    if !analysis.is_fast_forward() {
        return Err(msg(format!(
            "Can't fast-forward: {branch} has {ahead} commit(s) that {upstream_name} doesn't, and {upstream_name} has {behind} new. Merge or rebase in your git tool."
        )));
    }

    let before: Vec<(String, Option<git2::Oid>)> =
        repo.submodules()?.iter().map(|s| (s.path().to_string_lossy().replace('\\', "/"), s.head_id())).collect();

    // Safe checkout refuses to overwrite local changes, and changes nothing if it refuses.
    let obj = repo.find_object(target, None)?;
    repo.checkout_tree(&obj, Some(CheckoutBuilder::new().safe())).map_err(|e| {
        msg(format!("Pull stopped: it would overwrite local changes ({}). Commit or stash them first.", e.message()))
    })?;
    repo.find_reference(&refname)?.set_target(target, &format!("hangar: fast-forward to {upstream_name}"))?;

    let mut out = done(behind);
    for s in repo.submodules()? {
        let p = s.path().to_string_lossy().replace('\\', "/");
        if before.iter().any(|(bp, id)| *bp == p && *id != s.head_id()) {
            out.moved_submodules.push(p);
        }
    }
    Ok(out)
}

pub fn init_repo(path: &Path) -> AppResult<()> {
    let mut o = git2::RepositoryInitOptions::new();
    o.initial_head("main");
    Repository::init_opts(path, &o)?;
    Ok(())
}

pub fn libgit2_version() -> String {
    let (a, b, c) = git2::Version::get().libgit2_version();
    format!("{a}.{b}.{c}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn commit_all(repo: &Repository, msg: &str) {
        let mut idx = repo.index().unwrap();
        idx.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None).unwrap();
        idx.write().unwrap();
        let tree = repo.find_tree(idx.write_tree().unwrap()).unwrap();
        let sig = git2::Signature::now("t", "t@example.com").unwrap();
        let parents: Vec<git2::Commit> = repo.head().ok().and_then(|h| h.peel_to_commit().ok()).into_iter().collect();
        let refs: Vec<&git2::Commit> = parents.iter().collect();
        repo.commit(Some("HEAD"), &sig, &sig, msg, &tree, &refs).unwrap();
    }

    fn noop() -> ProgressFn {
        Arc::new(|_| {})
    }

    #[test]
    fn status_counts_changes_and_unpushed() {
        let t = tempfile::tempdir().unwrap();
        init_repo(t.path()).unwrap();
        let s = status(t.path()).unwrap();
        assert!(s.unborn);
        assert_eq!(s.branch.as_deref(), Some("main"));

        let repo = Repository::open(t.path()).unwrap();
        fs::write(t.path().join("a.txt"), "1").unwrap();
        commit_all(&repo, "first");
        fs::write(t.path().join("a.txt"), "2").unwrap();
        fs::write(t.path().join("b.txt"), "new").unwrap();

        let s = status(t.path()).unwrap();
        assert!(!s.unborn);
        assert_eq!(s.modified, 1);
        assert_eq!(s.untracked, 1);
        assert_eq!(s.unpushed, 1, "no remote, so every commit is unpushed");
        assert_eq!(s.last_commit.unwrap().summary, "first");
    }

    #[test]
    fn local_clone_and_cleanup_on_failure() {
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("src");
        fs::create_dir_all(&src).unwrap();
        init_repo(&src).unwrap();
        fs::write(src.join("x.txt"), "x").unwrap();
        commit_all(&Repository::open(&src).unwrap(), "c1");

        let dest = t.path().join("dest");
        clone_repo(src.to_str().unwrap(), &dest, None, true, None, false, Arc::new(AtomicBool::new(false)), noop()).unwrap();
        assert_eq!(fs::read_to_string(dest.join("x.txt")).unwrap(), "x");
        let s = status(&dest).unwrap();
        assert_eq!(s.unpushed, 0, "freshly cloned commits are on origin");
        assert_eq!(s.ahead, Some(0));

        // Existing destination is refused and left alone.
        assert!(clone_repo(src.to_str().unwrap(), &dest, None, false, None, false, Arc::new(AtomicBool::new(false)), noop()).is_err());
        assert!(dest.join("x.txt").exists());

        // A failing clone leaves nothing behind.
        let bad = t.path().join("bad");
        assert!(clone_repo(t.path().join("nope").to_str().unwrap(), &bad, None, false, None, false, Arc::new(AtomicBool::new(false)), noop()).is_err());
        assert!(!bad.exists());
    }

    /// Real HTTPS clone from GitHub through the bundled libgit2 (WinHTTP on Windows).
    /// Needs network: `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn clones_public_github_repo_over_https() {
        let t = tempfile::tempdir().unwrap();
        let dest = t.path().join("octocat__Hello-World");
        let seen = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let seen2 = seen.clone();
        let on: ProgressFn = Arc::new(move |p| seen2.lock().unwrap().push(p.stage));
        clone_repo("https://github.com/octocat/Hello-World.git", &dest, None, true, None, false, Arc::new(AtomicBool::new(false)), on).unwrap();
        assert!(dest.join("README").exists());
        let s = status(&dest).unwrap();
        assert_eq!(s.unpushed, 0);
        assert_eq!(s.remotes[0].url, "https://github.com/octocat/Hello-World.git");
        assert!(seen.lock().unwrap().iter().any(|s| s == "receiving"));
    }

    #[test]
    fn lists_submodules_with_their_status() {
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("lib-src");
        fs::create_dir_all(&src).unwrap();
        init_repo(&src).unwrap();
        fs::write(src.join("lib.txt"), "v1").unwrap();
        commit_all(&Repository::open(&src).unwrap(), "lib v1");

        let parent = t.path().join("parent");
        fs::create_dir_all(&parent).unwrap();
        init_repo(&parent).unwrap();
        let prepo = Repository::open(&parent).unwrap();
        fs::write(parent.join("main.txt"), "m").unwrap();
        commit_all(&prepo, "parent");
        let mut sm = prepo.submodule(src.to_str().unwrap(), Path::new("libs/lib"), true).unwrap();
        sm.clone(None).unwrap();
        sm.add_finalize().unwrap();
        commit_all(&prepo, "add submodule");

        let subs = submodules(&parent).unwrap();
        assert_eq!(subs.len(), 1);
        let s = &subs[0];
        assert_eq!(s.path, "libs/lib");
        assert!(s.initialized);
        assert!(!s.moved);
        assert_eq!(s.status.as_ref().unwrap().last_commit.as_ref().unwrap().summary, "lib v1");

        // Uncommitted work inside the submodule shows up in its own status.
        fs::write(parent.join("libs/lib/lib.txt"), "v2").unwrap();
        let s = &submodules(&parent).unwrap()[0];
        assert_eq!(s.status.as_ref().unwrap().modified, 1);
    }

    /// A parent repo with one submodule pinned to `lib v1`.
    fn parent_with_submodule(t: &Path) -> (PathBuf, PathBuf) {
        let src = t.join("lib-src");
        fs::create_dir_all(&src).unwrap();
        init_repo(&src).unwrap();
        fs::write(src.join("lib.txt"), "v1").unwrap();
        commit_all(&Repository::open(&src).unwrap(), "lib v1");
        let parent = t.join("parent");
        fs::create_dir_all(&parent).unwrap();
        init_repo(&parent).unwrap();
        let prepo = Repository::open(&parent).unwrap();
        fs::write(parent.join("main.txt"), "m").unwrap();
        commit_all(&prepo, "parent");
        let mut sm = prepo.submodule(src.to_str().unwrap(), Path::new("libs/lib"), true).unwrap();
        sm.clone(None).unwrap();
        sm.add_finalize().unwrap();
        commit_all(&prepo, "add submodule");
        (src, parent)
    }

    #[test]
    fn cloned_submodules_end_up_on_their_branch() {
        let t = tempfile::tempdir().unwrap();
        let (_src, parent) = parent_with_submodule(t.path());
        let dest = t.path().join("clone");
        clone_repo(parent.to_str().unwrap(), &dest, None, true, None, false, Arc::new(AtomicBool::new(false)), noop()).unwrap();
        let s = &submodules(&dest).unwrap()[0];
        let st = s.status.as_ref().unwrap();
        assert!(!st.detached, "submodule should not be detached");
        assert_eq!(st.branch.as_deref(), Some("main"));
        assert_eq!(st.upstream.as_deref(), Some("origin/main"));
        assert!(!s.moved);
    }

    #[test]
    fn attach_keeps_the_pinned_commit_when_branch_moved_on() {
        let t = tempfile::tempdir().unwrap();
        let (src, parent) = parent_with_submodule(t.path());
        // The library gets a new commit after the parent pinned v1.
        fs::write(src.join("lib.txt"), "v2").unwrap();
        commit_all(&Repository::open(&src).unwrap(), "lib v2");

        let dest = t.path().join("clone");
        clone_repo(parent.to_str().unwrap(), &dest, None, true, None, false, Arc::new(AtomicBool::new(false)), noop()).unwrap();
        let s = &submodules(&dest).unwrap()[0];
        let st = s.status.as_ref().unwrap();
        assert_eq!(st.branch.as_deref(), Some("main"));
        assert_eq!(st.behind, Some(1), "local main stays at the pinned commit, one behind origin");
        assert_eq!(fs::read_to_string(dest.join("libs/lib/lib.txt")).unwrap(), "v1");
        assert!(!s.moved);
    }

    #[test]
    fn attach_leaves_local_work_alone() {
        let t = tempfile::tempdir().unwrap();
        let (_src, parent) = parent_with_submodule(t.path());
        let sub = parent.join("libs/lib");
        let repo = Repository::open(&sub).unwrap();
        // Detach, then give local main a commit that isn't on the remote.
        let head = repo.head().unwrap().target().unwrap();
        repo.set_head_detached(head).unwrap();
        fs::write(sub.join("local.txt"), "x").unwrap();
        let sig = git2::Signature::now("t", "t@example.com").unwrap();
        let mut idx = repo.index().unwrap();
        idx.add_path(Path::new("local.txt")).unwrap();
        let tree = repo.find_tree(idx.write_tree().unwrap()).unwrap();
        let parent_c = repo.find_commit(head).unwrap();
        repo.commit(Some("refs/heads/main"), &sig, &sig, "local only", &tree, &[&parent_c]).unwrap();

        let out = attach_submodules(&parent, None).unwrap();
        assert_eq!(out[0].branch, None);
        assert!(out[0].note.as_ref().unwrap().contains("left alone"));
        assert!(repo.head_detached().unwrap());
    }

    /// An upstream repo with one commit, and a clone of it.
    fn upstream_and_clone(t: &Path) -> (PathBuf, PathBuf) {
        let up = t.join("upstream");
        fs::create_dir_all(&up).unwrap();
        init_repo(&up).unwrap();
        fs::write(up.join("a.txt"), "1").unwrap();
        commit_all(&Repository::open(&up).unwrap(), "c1");
        let clone = t.join("clone");
        clone_repo(up.to_str().unwrap(), &clone, None, false, None, false, Arc::new(AtomicBool::new(false)), noop()).unwrap();
        (up, clone)
    }

    #[test]
    fn fetch_then_fast_forward_pull() {
        let t = tempfile::tempdir().unwrap();
        let (up, clone) = upstream_and_clone(t.path());
        fs::write(up.join("a.txt"), "2").unwrap();
        commit_all(&Repository::open(&up).unwrap(), "c2");

        fetch_all(&clone, None).unwrap();
        let s = status(&clone).unwrap();
        assert_eq!(s.behind, Some(1));
        assert_eq!(fs::read_to_string(clone.join("a.txt")).unwrap(), "1", "fetch doesn't touch files");

        let out = pull_ff(&clone, None).unwrap();
        assert_eq!(out.commits, 1);
        assert_eq!(fs::read_to_string(clone.join("a.txt")).unwrap(), "2");
        let s = status(&clone).unwrap();
        assert_eq!((s.ahead, s.behind), (Some(0), Some(0)));
        assert_eq!(s.modified + s.staged, 0);

        assert_eq!(pull_ff(&clone, None).unwrap().commits, 0, "second pull is up to date");
    }

    #[test]
    fn pull_refuses_to_merge_or_overwrite() {
        let t = tempfile::tempdir().unwrap();
        let (up, clone) = upstream_and_clone(t.path());
        fs::write(up.join("a.txt"), "2").unwrap();
        commit_all(&Repository::open(&up).unwrap(), "c2");

        // Local edit to the same file: pull must stop and keep it.
        fs::write(clone.join("a.txt"), "mine").unwrap();
        let e = pull_ff(&clone, None).unwrap_err().to_string();
        assert!(e.contains("overwrite local changes"), "{e}");
        assert_eq!(fs::read_to_string(clone.join("a.txt")).unwrap(), "mine");
        assert_eq!(status(&clone).unwrap().behind, Some(1), "branch not moved");

        // A local commit makes the histories diverge: no merge.
        commit_all(&Repository::open(&clone).unwrap(), "local");
        let e = pull_ff(&clone, None).unwrap_err().to_string();
        assert!(e.contains("Can't fast-forward"), "{e}");
    }

    #[test]
    fn pull_needs_a_branch() {
        let t = tempfile::tempdir().unwrap();
        let (_up, clone) = upstream_and_clone(t.path());
        let repo = Repository::open(&clone).unwrap();
        repo.set_head_detached(repo.head().unwrap().target().unwrap()).unwrap();
        assert!(pull_ff(&clone, None).unwrap_err().to_string().contains("Detached HEAD"));
    }

    #[test]
    fn clones_paths_longer_than_windows_limit() {
        enable_long_paths(&std::env::temp_dir().join("hangar-test-git-config"));
        let t = tempfile::tempdir().unwrap();
        let src = t.path().join("src");
        // 12 levels of 31 characters: the file path is well over 260 characters.
        let deep: PathBuf = (0..12).map(|i| format!("directory-level-{i:02}-xxxxxxxxxx")).collect();
        fs::create_dir_all(src.join(&deep)).unwrap();
        fs::write(src.join(&deep).join("file.txt"), "deep").unwrap();
        assert!(src.join(&deep).join("file.txt").to_string_lossy().len() > 380);
        init_repo(&src).unwrap();
        commit_all(&Repository::open(&src).unwrap(), "deep file");

        let dest = t.path().join("dest");
        clone_repo(src.to_str().unwrap(), &dest, None, false, None, false, Arc::new(AtomicBool::new(false)), noop()).unwrap();
        assert_eq!(fs::read_to_string(dest.join(&deep).join("file.txt")).unwrap(), "deep");
    }

    #[test]
    fn fetch_keeps_a_single_branch_clone_single_branch() {
        let t = tempfile::tempdir().unwrap();
        let (up, clone) = upstream_and_clone(t.path());
        // Upstream gets a second branch.
        let urepo = Repository::open(&up).unwrap();
        urepo.branch("feat", &urepo.head().unwrap().peel_to_commit().unwrap(), false).unwrap();
        // Narrow origin to main only, like a "latest commit only" clone.
        let repo = Repository::open(&clone).unwrap();
        repo.config().unwrap().set_str("remote.origin.fetch", "+refs/heads/main:refs/remotes/origin/main").unwrap();
        let _ = repo.find_reference("refs/remotes/origin/feat").map(|mut r| r.delete());
        assert!(status(&clone).unwrap().single_branch);

        fetch_all(&clone, None).unwrap();
        assert!(repo.find_reference("refs/remotes/origin/feat").is_err(), "single-branch fetch must not pull other branches");

        full_history(&clone, None).unwrap();
        assert!(repo.find_reference("refs/remotes/origin/feat").is_ok(), "full history brings every branch");
        assert!(!status(&clone).unwrap().single_branch);
    }

    /// Real shallow clone from GitHub: `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn shallow_clone_from_github_and_upgrade_to_full() {
        let t = tempfile::tempdir().unwrap();
        let dest = t.path().join("hello");
        clone_repo("https://github.com/octocat/Hello-World.git", &dest, None, false, None, true, Arc::new(AtomicBool::new(false)), noop()).unwrap();
        let s = status(&dest).unwrap();
        assert!(s.shallow && s.single_branch, "shallow={} single={}", s.shallow, s.single_branch);
        assert_eq!(s.branch.as_deref(), Some("master"));
        // Remote branches, not counting the origin/HEAD pointer.
        let count = |p: &Path| {
            let repo = Repository::open(p).unwrap();
            let n = repo
                .branches(Some(BranchType::Remote))
                .unwrap()
                .flatten()
                .filter(|(b, _)| !b.name().ok().flatten().is_some_and(|n| n.ends_with("/HEAD")))
                .count();
            n
        };
        assert_eq!(count(&dest), 1);

        full_history(&dest, None).unwrap();
        let s = status(&dest).unwrap();
        assert!(!s.shallow && !s.single_branch);
        assert!(count(&dest) > 1);
    }

    #[test]
    fn ssh_urls_are_fetched_over_https() {
        assert_eq!(https_for("git@github.com:a/b.git").as_deref(), Some("https://github.com/a/b.git"));
        assert_eq!(https_for("https://github.com/a/b.git"), None);
        assert_eq!(https_for("C:\\repos\\x"), None);
    }

    #[test]
    fn resolves_relative_submodule_urls() {
        let parent = crate::naming::parse_repo_input("HKCC-KTFC/KTFC-Stack").unwrap();
        let r = resolve_submodule_url("../api.git", Some(&parent)).unwrap();
        assert_eq!((r.owner.as_str(), r.repo.as_str()), ("HKCC-KTFC", "api"));
        let r = resolve_submodule_url("../../other/web", Some(&parent)).unwrap();
        assert_eq!((r.owner.as_str(), r.repo.as_str()), ("other", "web"));
        let r = resolve_submodule_url("https://github.com/a/b.git", None).unwrap();
        assert_eq!((r.owner.as_str(), r.repo.as_str()), ("a", "b"));
        assert!(resolve_submodule_url("../api.git", None).is_none());
    }

    #[test]
    fn redacts_credentials() {
        assert_eq!(redact_url("https://user:tok@github.com/a/b.git"), "https://github.com/a/b.git");
        assert_eq!(redact_url("https://github.com/a/b.git"), "https://github.com/a/b.git");
        assert_eq!(redact_url("git@github.com:a/b.git"), "git@github.com:a/b.git");
    }
}
