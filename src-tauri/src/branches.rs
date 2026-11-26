//! Branches: list, switch, check out a remote branch, create, fast-forward,
//! delete. Everything goes through libgit2 and refuses anything that could
//! lose work (overwriting local changes, deleting unpushed commits, merging).

use crate::error::{msg, AppResult};
use crate::git::{commit_info, default_remote_branch, fetch_remote, pull_ff, CommitInfo, PullOutcome};
use git2::build::CheckoutBuilder;
use git2::{Branch, BranchType, ObjectType, Oid, Repository};
use serde::Serialize;
use std::path::Path;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LocalBranch {
    pub name: String,
    pub current: bool,
    pub upstream: Option<String>,
    /// An upstream is configured but no longer exists on the remote.
    pub upstream_gone: bool,
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
    pub last_commit: Option<CommitInfo>,
    /// Deleting it loses nothing: every commit is on a remote or in the current branch.
    pub safe_to_delete: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct RemoteBranch {
    /// e.g. `origin/feat/x`
    pub name: String,
    pub remote: String,
    pub branch: String,
    pub last_commit: Option<CommitInfo>,
    /// The local branch that tracks it (or has its name), if any.
    pub local: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct BranchList {
    pub local: Vec<LocalBranch>,
    pub remote: Vec<RemoteBranch>,
    pub default_branch: Option<String>,
    pub detached: bool,
}

/// Does `tip` have commits that are on no remote branch and not in `head`?
fn has_unique_commits(repo: &Repository, tip: Oid, head: Option<Oid>) -> bool {
    let Ok(mut walk) = repo.revwalk() else { return true };
    if walk.push(tip).is_err() {
        return true;
    }
    if let Ok(refs) = repo.references() {
        for r in refs.flatten() {
            if r.name().is_ok_and(|n| n.starts_with("refs/remotes/")) {
                if let Some(o) = r.target() {
                    let _ = walk.hide(o);
                }
            }
        }
    }
    if let Some(h) = head {
        let _ = walk.hide(h);
    }
    walk.next().is_some()
}

/// Path of another worktree that has `refname` checked out, if any.
fn checked_out_elsewhere(repo: &Repository, refname: &str) -> Option<String> {
    let names = repo.worktrees().ok()?;
    for n in names.iter().flatten().flatten() {
        let Ok(wt) = repo.find_worktree(n) else { continue };
        let Ok(wrepo) = Repository::open_from_worktree(&wt) else { continue };
        if let Ok(h) = wrepo.find_reference("HEAD") {
            if h.symbolic_target().ok().flatten() == Some(refname) {
                return Some(wt.path().display().to_string());
            }
        }
    }
    None
}

pub fn list(path: &Path) -> AppResult<BranchList> {
    let repo = Repository::open(path)?;
    let detached = repo.head_detached().unwrap_or(false);
    let head = repo.head().ok().and_then(|h| h.target());
    let cfg = repo.config()?;

    let mut local = Vec::new();
    for item in repo.branches(Some(BranchType::Local))? {
        let (b, _) = item?;
        let Some(name) = b.name()?.map(String::from) else { continue };
        let tip = b.get().target();
        let (mut upstream, mut ahead, mut behind) = (None, None, None);
        if let Ok(up) = b.upstream() {
            upstream = up.name().ok().flatten().map(String::from);
            if let (Some(t), Some(u)) = (tip, up.get().target()) {
                if let Ok((a, bh)) = repo.graph_ahead_behind(t, u) {
                    ahead = Some(a as u32);
                    behind = Some(bh as u32);
                }
            }
        }
        let current = b.is_head() && !detached;
        local.push(LocalBranch {
            upstream_gone: upstream.is_none() && cfg.get_string(&format!("branch.{name}.merge")).is_ok(),
            safe_to_delete: !current && tip.is_some_and(|t| !has_unique_commits(&repo, t, head)),
            last_commit: commit_info(&repo, tip),
            name,
            current,
            upstream,
            ahead,
            behind,
        });
    }

    let mut remote = Vec::new();
    for item in repo.branches(Some(BranchType::Remote))? {
        let (b, _) = item?;
        if b.get().symbolic_target().ok().flatten().is_some() {
            continue; // origin/HEAD
        }
        let Some(full) = b.name()?.map(String::from) else { continue };
        let Some((r, name)) = full.split_once('/') else { continue };
        let tracking = local
            .iter()
            .find(|l| l.upstream.as_deref() == Some(full.as_str()) || l.name == name)
            .map(|l| l.name.clone());
        remote.push(RemoteBranch {
            remote: r.to_string(),
            branch: name.to_string(),
            last_commit: commit_info(&repo, b.get().target()),
            local: tracking,
            name: full.clone(),
        });
    }

    let time = |c: &Option<CommitInfo>| c.as_ref().and_then(|c| c.time);
    local.sort_by(|a, b| b.current.cmp(&a.current).then(time(&b.last_commit).cmp(&time(&a.last_commit))));
    remote.sort_by(|a, b| time(&b.last_commit).cmp(&time(&a.last_commit)));
    Ok(BranchList { local, remote, default_branch: default_remote_branch(&repo), detached })
}

/// Check out a local branch. Uncommitted changes that don't conflict come
/// along (like `git switch`); conflicting ones stop it before anything changes.
fn checkout_local(repo: &Repository, name: &str) -> AppResult<()> {
    let b = repo.find_branch(name, BranchType::Local).map_err(|_| msg(format!("There's no local branch called {name}.")))?;
    let refname = b.get().name()?.to_string();
    if let Some(p) = checked_out_elsewhere(repo, &refname) {
        return Err(msg(format!("{name} is checked out in another worktree ({p}).")));
    }
    let target = b.get().peel(ObjectType::Commit)?;
    repo.checkout_tree(&target, Some(CheckoutBuilder::new().safe())).map_err(|e| {
        msg(format!("Switching stopped: it would overwrite local changes ({}). Commit or stash them first.", e.message()))
    })?;
    repo.set_head(&refname)?;
    Ok(())
}

pub fn switch(path: &Path, name: &str) -> AppResult<()> {
    checkout_local(&Repository::open(path)?, name)
}

/// Check out `origin/x` as a local branch `x` that tracks it.
pub fn checkout_remote(path: &Path, remote_branch: &str) -> AppResult<String> {
    let repo = Repository::open(path)?;
    let (_, name) = remote_branch.split_once('/').ok_or_else(|| msg("Expected a remote branch like origin/name."))?;
    let rb = repo
        .find_branch(remote_branch, BranchType::Remote)
        .map_err(|_| msg(format!("{remote_branch} doesn't exist. Fetch first.")))?;
    let tip = rb.get().target().ok_or_else(|| msg(format!("{remote_branch} doesn't point at a commit.")))?;

    if let Ok(existing) = repo.find_branch(name, BranchType::Local) {
        let up = existing.upstream().ok().and_then(|u| u.name().ok().flatten().map(String::from));
        if up.as_deref() != Some(remote_branch) {
            return Err(msg(format!(
                "A local branch called {name} already exists{}. Switch to it instead.",
                up.map(|u| format!(" and tracks {u}")).unwrap_or_default()
            )));
        }
        checkout_local(&repo, name)?;
        return Ok(name.to_string());
    }

    let mut created = repo.branch(name, &repo.find_commit(tip)?, false)?;
    created.set_upstream(Some(remote_branch))?;
    if let Err(e) = checkout_local(&repo, name) {
        let _ = created.delete();
        return Err(e);
    }
    Ok(name.to_string())
}

/// Create a branch from `from` (a branch or commit; default: HEAD) and optionally switch to it.
pub fn create(path: &Path, name: &str, from: Option<&str>, switch_to: bool) -> AppResult<String> {
    let name = name.trim();
    if name.is_empty() || !Branch::name_is_valid(name)? {
        return Err(msg(format!("\"{name}\" isn't a valid branch name.")));
    }
    let repo = Repository::open(path)?;
    if repo.find_branch(name, BranchType::Local).is_ok() {
        return Err(msg(format!("A branch called {name} already exists.")));
    }
    let start = match from.map(str::trim).filter(|f| !f.is_empty()) {
        Some(f) => repo.revparse_single(f).map_err(|_| msg(format!("Couldn't find {f}.")))?.peel_to_commit()?,
        None => repo.head().map_err(|_| msg("This repo has no commits yet."))?.peel_to_commit()?,
    };
    repo.branch(name, &start, false)?;
    if switch_to {
        checkout_local(&repo, name).map_err(|e| msg(format!("Created {name}, but couldn't switch to it: {e}")))?;
    }
    Ok(name.to_string())
}

/// Fast-forward a local branch to its upstream. The current branch goes
/// through a normal pull; other branches just have their pointer moved.
pub fn update(path: &Path, name: &str, token: Option<String>) -> AppResult<PullOutcome> {
    let repo = Repository::open(path)?;
    let b = repo.find_branch(name, BranchType::Local).map_err(|_| msg(format!("There's no local branch called {name}.")))?;
    if b.is_head() && !repo.head_detached()? {
        drop(b);
        drop(repo);
        return pull_ff(path, token);
    }
    let refname = b.get().name()?.to_string();
    if let Some(p) = checked_out_elsewhere(&repo, &refname) {
        return Err(msg(format!("{name} is checked out in another worktree ({p}); update it there.")));
    }
    let upstream = b
        .upstream()
        .map_err(|_| msg(format!("{name} has no upstream branch.")))?
        .name()?
        .unwrap_or_default()
        .to_string();
    drop(b);
    let remote = repo.branch_upstream_remote(&refname)?;
    let remote = remote.as_str().map_err(|_| msg("The upstream remote's name isn't valid text."))?.to_string();
    fetch_remote(&repo, &remote, &token)?;

    let b = repo.find_branch(name, BranchType::Local)?;
    let local = b.get().target().ok_or_else(|| msg(format!("{name} doesn't point at a commit.")))?;
    let target = b.upstream()?.get().target().ok_or_else(|| msg(format!("{upstream} doesn't point at a commit.")))?;
    let done = |commits| PullOutcome { branch: name.to_string(), upstream: upstream.clone(), commits, moved_submodules: Vec::new() };
    if local == target {
        return Ok(done(0));
    }
    let (ahead, behind) = repo.graph_ahead_behind(local, target)?;
    if ahead > 0 {
        return Err(msg(format!(
            "Can't fast-forward: {name} has {ahead} commit(s) that {upstream} doesn't, and {upstream} has {behind} new. Merge or rebase in your git tool."
        )));
    }
    repo.find_reference(&refname)?.set_target(target, &format!("hangar: fast-forward to {upstream}"))?;
    Ok(done(behind))
}

/// Delete a local branch, only if that loses no commits.
pub fn delete(path: &Path, name: &str) -> AppResult<()> {
    let repo = Repository::open(path)?;
    let mut b = repo.find_branch(name, BranchType::Local).map_err(|_| msg(format!("There's no local branch called {name}.")))?;
    if b.is_head() && !repo.head_detached()? {
        return Err(msg("You can't delete the branch you're on. Switch to another branch first."));
    }
    let refname = b.get().name()?.to_string();
    if let Some(p) = checked_out_elsewhere(&repo, &refname) {
        return Err(msg(format!("{name} is checked out in another worktree ({p}).")));
    }
    let tip = b.get().target().ok_or_else(|| msg(format!("{name} doesn't point at a commit.")))?;
    let head = repo.head().ok().and_then(|h| h.target());
    if has_unique_commits(&repo, tip, head) {
        return Err(msg(format!(
            "{name} has commits that aren't on any remote or in the current branch. Push or merge them first."
        )));
    }
    b.delete()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::{clone_repo, init_repo, status, ProgressFn};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;

    fn commit_all(repo: &Repository, m: &str) {
        let mut idx = repo.index().unwrap();
        idx.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None).unwrap();
        idx.write().unwrap();
        let tree = repo.find_tree(idx.write_tree().unwrap()).unwrap();
        let sig = git2::Signature::now("t", "t@example.com").unwrap();
        let parents: Vec<git2::Commit> = repo.head().ok().and_then(|h| h.peel_to_commit().ok()).into_iter().collect();
        let refs: Vec<&git2::Commit> = parents.iter().collect();
        repo.commit(Some("HEAD"), &sig, &sig, m, &tree, &refs).unwrap();
    }

    /// Upstream with `main` (a.txt=main) and `feat` (a.txt=feat, one commit ahead), and a clone of it.
    fn setup(t: &Path) -> (PathBuf, PathBuf) {
        let up = t.join("up");
        fs::create_dir_all(&up).unwrap();
        init_repo(&up).unwrap();
        let r = Repository::open(&up).unwrap();
        fs::write(up.join("a.txt"), "main").unwrap();
        commit_all(&r, "c1");
        let c1 = r.head().unwrap().peel_to_commit().unwrap();
        r.branch("feat", &c1, false).unwrap();
        r.set_head("refs/heads/feat").unwrap();
        fs::write(up.join("a.txt"), "feat").unwrap();
        commit_all(&r, "feat work");
        r.set_head("refs/heads/main").unwrap();
        r.checkout_head(Some(CheckoutBuilder::new().force())).unwrap();
        let clone = t.join("clone");
        let noop: ProgressFn = Arc::new(|_| {});
        clone_repo(up.to_str().unwrap(), &clone, None, false, None, false, Arc::new(AtomicBool::new(false)), noop).unwrap();
        (up, clone)
    }

    #[test]
    fn lists_local_and_remote_branches() {
        let t = tempfile::tempdir().unwrap();
        let (_up, clone) = setup(t.path());
        let l = list(&clone).unwrap();
        assert_eq!(l.local.len(), 1);
        assert!(l.local[0].current && l.local[0].name == "main");
        assert_eq!(l.local[0].upstream.as_deref(), Some("origin/main"));
        let feat = l.remote.iter().find(|r| r.name == "origin/feat").unwrap();
        assert_eq!(feat.local, None);
        assert_eq!(l.remote.iter().find(|r| r.name == "origin/main").unwrap().local.as_deref(), Some("main"));
    }

    #[test]
    fn checkout_remote_then_switch_back() {
        let t = tempfile::tempdir().unwrap();
        let (_up, clone) = setup(t.path());
        assert_eq!(checkout_remote(&clone, "origin/feat").unwrap(), "feat");
        let s = status(&clone).unwrap();
        assert_eq!(s.branch.as_deref(), Some("feat"));
        assert_eq!(s.upstream.as_deref(), Some("origin/feat"));
        assert_eq!(fs::read_to_string(clone.join("a.txt")).unwrap(), "feat");

        switch(&clone, "main").unwrap();
        assert_eq!(fs::read_to_string(clone.join("a.txt")).unwrap(), "main");
        // Checking out origin/feat again just switches to the existing tracking branch.
        assert_eq!(checkout_remote(&clone, "origin/feat").unwrap(), "feat");
    }

    #[test]
    fn switch_refuses_to_overwrite_local_changes() {
        let t = tempfile::tempdir().unwrap();
        let (_up, clone) = setup(t.path());
        checkout_remote(&clone, "origin/feat").unwrap();
        switch(&clone, "main").unwrap();
        fs::write(clone.join("a.txt"), "edited").unwrap();
        let e = switch(&clone, "feat").unwrap_err().to_string();
        assert!(e.contains("overwrite local changes"), "{e}");
        assert_eq!(fs::read_to_string(clone.join("a.txt")).unwrap(), "edited");
        assert_eq!(status(&clone).unwrap().branch.as_deref(), Some("main"));
        // A failed checkout of a new remote branch leaves no half-made branch behind.
        let t2 = tempfile::tempdir().unwrap();
        let (_up2, clone2) = setup(t2.path());
        fs::write(clone2.join("a.txt"), "edited").unwrap();
        assert!(checkout_remote(&clone2, "origin/feat").is_err());
        assert!(Repository::open(&clone2).unwrap().find_branch("feat", BranchType::Local).is_err());
    }

    #[test]
    fn create_validates_and_switches() {
        let t = tempfile::tempdir().unwrap();
        let (_up, clone) = setup(t.path());
        assert!(create(&clone, "bad..name", None, true).is_err());
        assert!(create(&clone, "main", None, true).is_err());
        create(&clone, "topic", None, true).unwrap();
        assert_eq!(status(&clone).unwrap().branch.as_deref(), Some("topic"));
        create(&clone, "from-feat", Some("origin/feat"), false).unwrap();
        assert_eq!(status(&clone).unwrap().branch.as_deref(), Some("topic"), "not switched");
    }

    #[test]
    fn update_fast_forwards_a_branch_that_is_not_checked_out() {
        let t = tempfile::tempdir().unwrap();
        let (up, clone) = setup(t.path());
        checkout_remote(&clone, "origin/feat").unwrap();
        switch(&clone, "main").unwrap();
        // New commit on upstream feat.
        let r = Repository::open(&up).unwrap();
        r.set_head("refs/heads/feat").unwrap();
        r.checkout_head(Some(CheckoutBuilder::new().force())).unwrap();
        fs::write(up.join("a.txt"), "feat 2").unwrap();
        commit_all(&r, "feat 2");

        let out = update(&clone, "feat", None).unwrap();
        assert_eq!(out.commits, 1);
        assert_eq!(fs::read_to_string(clone.join("a.txt")).unwrap(), "main", "working tree untouched");
        let l = list(&clone).unwrap();
        let feat = l.local.iter().find(|b| b.name == "feat").unwrap();
        assert_eq!((feat.ahead, feat.behind), (Some(0), Some(0)));
    }

    #[test]
    fn delete_only_when_nothing_is_lost() {
        let t = tempfile::tempdir().unwrap();
        let (_up, clone) = setup(t.path());
        checkout_remote(&clone, "origin/feat").unwrap();
        assert!(delete(&clone, "feat").is_err(), "can't delete the current branch");
        switch(&clone, "main").unwrap();
        assert!(list(&clone).unwrap().local.iter().find(|b| b.name == "feat").unwrap().safe_to_delete);
        delete(&clone, "feat").unwrap();

        // A branch with a commit that exists nowhere else is refused.
        create(&clone, "wip", None, true).unwrap();
        fs::write(clone.join("b.txt"), "x").unwrap();
        commit_all(&Repository::open(&clone).unwrap(), "local only");
        switch(&clone, "main").unwrap();
        assert!(!list(&clone).unwrap().local.iter().find(|b| b.name == "wip").unwrap().safe_to_delete);
        assert!(delete(&clone, "wip").unwrap_err().to_string().contains("Push or merge"));
    }

    #[test]
    fn clone_a_specific_branch() {
        let t = tempfile::tempdir().unwrap();
        let (up, _clone) = setup(t.path());
        let dest = t.path().join("clone-feat");
        let noop: ProgressFn = Arc::new(|_| {});
        clone_repo(up.to_str().unwrap(), &dest, None, false, Some("feat"), false, Arc::new(AtomicBool::new(false)), noop.clone()).unwrap();
        assert_eq!(status(&dest).unwrap().branch.as_deref(), Some("feat"));
        let e = clone_repo(up.to_str().unwrap(), &t.path().join("x"), None, false, Some("nope"), false, Arc::new(AtomicBool::new(false)), noop)
            .unwrap_err()
            .to_string();
        assert!(e.contains("no branch called nope"), "{e}");
    }
}
