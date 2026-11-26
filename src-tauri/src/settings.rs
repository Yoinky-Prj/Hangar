//! App-level settings (not tied to a workspace), stored in the OS config dir.

use crate::error::{msg, AppResult};
use crate::meta::write_json_atomic;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// Every workspace the user has added, in the order they were added.
    /// (Older versions kept the last 8 opened as `recentWorkspaces`.)
    #[serde(default, alias = "recentWorkspaces")]
    pub workspaces: Vec<String>,
    #[serde(default)]
    pub last_workspace: Option<String>,
    /// Client ID of the user's own GitHub OAuth App (for device-code sign-in).
    #[serde(default)]
    pub github_client_id: Option<String>,
}

/// Windows paths: case-insensitive, either slash, trailing slash ignored.
pub fn same_path(a: &str, b: &str) -> bool {
    let n = |p: &str| p.trim_end_matches(['\\', '/']).replace('/', "\\").to_lowercase();
    n(a) == n(b)
}

fn path(app: &AppHandle) -> AppResult<PathBuf> {
    app.path().app_config_dir().map(|d| d.join("settings.json")).map_err(|e| msg(format!("No config folder: {e}")))
}

pub fn load(app: &AppHandle) -> Settings {
    path(app)
        .ok()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(app: &AppHandle, s: &Settings) -> AppResult<()> {
    write_json_atomic(&path(app)?, s)?;
    crate::jumplist::refresh(s.workspaces.clone());
    Ok(())
}

/// Add a workspace to the list (at the end, so the list doesn't reshuffle
/// every time you switch) and make it the one opened at startup.
pub fn remember_workspace(app: &AppHandle, ws: &str) -> AppResult<Settings> {
    let mut s = load(app);
    if !s.workspaces.iter().any(|p| same_path(p, ws)) {
        s.workspaces.push(ws.to_string());
    }
    s.last_workspace = Some(ws.to_string());
    save(app, &s)?;
    Ok(s)
}

/// What a plain start reopens: the workspace the last closed window showed,
/// or the start screen (None).
pub fn set_last_workspace(app: &AppHandle, ws: Option<&str>) -> AppResult<()> {
    let mut s = load(app);
    if s.last_workspace.as_deref() == ws {
        return Ok(());
    }
    s.last_workspace = ws.map(str::to_string);
    save(app, &s)
}

pub fn forget_workspace(app: &AppHandle, ws: &str) -> AppResult<Settings> {
    let mut s = load(app);
    s.workspaces.retain(|p| !same_path(p, ws));
    if s.last_workspace.as_deref().is_some_and(|l| same_path(l, ws)) {
        s.last_workspace = None;
    }
    save(app, &s)?;
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_old_recent_list() {
        let s: Settings = serde_json::from_str(r#"{"recentWorkspaces":["D:\\Workspaces"],"lastWorkspace":"D:\\Workspaces"}"#).unwrap();
        assert_eq!(s.workspaces, vec!["D:\\Workspaces".to_string()]);
        let out = serde_json::to_string(&s).unwrap();
        assert!(out.contains("\"workspaces\"") && !out.contains("recentWorkspaces"));
    }

    #[test]
    fn same_path_ignores_case_slashes_and_trailing_separator() {
        assert!(same_path("D:\\Workspaces", "d:/workspaces/"));
        assert!(!same_path("D:\\Workspaces", "D:\\Workspaces2"));
    }
}

pub fn set_client_id(app: &AppHandle, id: &str) -> AppResult<Settings> {
    let mut s = load(app);
    let id = id.trim();
    s.github_client_id = if id.is_empty() { None } else { Some(id.to_string()) };
    save(app, &s)?;
    Ok(s)
}
