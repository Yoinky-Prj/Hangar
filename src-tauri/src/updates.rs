//! Updates for the installed Hangar: it reads the latest GitHub release's
//! `latest.json` at start (and every few hours) and, when asked, downloads that
//! release's installer and runs it in update mode. The standalone exe and dev
//! builds never update themselves.
//!
//! `latest.json` is a release file (written by the release workflow), not the
//! GitHub API: the API allows only 60 requests an hour per IP address without
//! sign-in, which a shared network uses up quickly.
//!
//! There's no signature check: the installer is trusted because it is the one
//! attached to this repository's latest GitHub release, fetched over HTTPS.

use crate::error::{msg, AppResult};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};

/// `latest.json` of the latest published release; drafts and pre-releases
/// never count. A test build can point this at a local server.
const LATEST: &str = match option_env!("HANGAR_LATEST_RELEASE_URL") {
    Some(u) => u,
    None => "https://github.com/Yoinky-Prj/Hangar/releases/latest/download/latest.json",
};
const RECHECK: Duration = Duration::from_secs(6 * 60 * 60);

/// What the release workflow writes to `latest.json`.
#[derive(Deserialize)]
struct Latest {
    version: String,
    notes: Option<String>,
    pub_date: Option<String>,
    /// The installer.
    url: String,
    size: u64,
}

#[derive(Serialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    version: String,
    notes: Option<String>,
    date: Option<String>,
}

#[derive(Clone)]
struct Pending {
    info: UpdateInfo,
    url: String,
    size: u64,
}

pub struct Updates {
    http: reqwest::Client,
    pending: Mutex<Option<Pending>>,
}

impl Default for Updates {
    fn default() -> Self {
        Updates { http: crate::github::client(), pending: Mutex::new(None) }
    }
}

impl Updates {
    fn pending(&self) -> MutexGuard<'_, Option<Pending>> {
        self.pending.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    current: String,
    /// Installed with the installer, so it can update itself.
    installed: bool,
    available: Option<UpdateInfo>,
}

/// The installer puts uninstall.exe next to hangar.exe; the standalone exe
/// has none.
pub fn installed() -> bool {
    !cfg!(debug_assertions)
        && std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("uninstall.exe").is_file())).unwrap_or(false)
}

/// Where the downloaded installer goes; cleared before each download and at start.
fn download_dir() -> PathBuf {
    std::env::temp_dir().join("Hangar-update")
}

/// The release, if it's newer than `current`.
fn newer(latest: &Latest, current: &str) -> Option<Pending> {
    let version = semver::Version::parse(latest.version.trim_start_matches('v')).ok()?;
    if version <= semver::Version::parse(current).ok()? {
        return None;
    }
    Some(Pending {
        info: UpdateInfo {
            version: version.to_string(),
            notes: latest.notes.clone().filter(|n| !n.trim().is_empty()),
            date: latest.pub_date.clone(),
        },
        url: latest.url.clone(),
        size: latest.size,
    })
}

async fn check(app: &AppHandle) -> AppResult<Option<UpdateInfo>> {
    let updates = app.state::<Updates>();
    let res = updates.http.get(LATEST).send().await?;
    // No release yet, or the newest one is still building (its files come
    // some minutes after it's published): nothing to update to.
    let found = if res.status() == reqwest::StatusCode::NOT_FOUND {
        None
    } else {
        let latest: Latest = res.error_for_status()?.json().await?;
        newer(&latest, &app.package_info().version.to_string())
    };
    let info = found.as_ref().map(|p| p.info.clone());
    *updates.pending() = found;
    Ok(info)
}

/// Checks now and every few hours; every window hears about a new version.
pub fn watch(app: &AppHandle) {
    if !installed() {
        return;
    }
    // The installer of the update that just ran.
    let _ = std::fs::remove_dir_all(download_dir());
    let app = app.clone();
    std::thread::spawn(move || {
        let mut told: Option<UpdateInfo> = None;
        loop {
            match tauri::async_runtime::block_on(check(&app)) {
                Ok(Some(found)) if told.as_ref() != Some(&found) => {
                    let _ = app.emit("update-available", &found);
                    told = Some(found);
                }
                Ok(_) => {}
                Err(e) => eprintln!("{e}"),
            }
            std::thread::sleep(RECHECK);
        }
    });
}

#[tauri::command]
pub fn update_status(app: AppHandle) -> UpdateStatus {
    UpdateStatus {
        current: app.package_info().version.to_string(),
        installed: installed(),
        available: app.state::<Updates>().pending().as_ref().map(|p| p.info.clone()),
    }
}

#[tauri::command]
pub async fn check_for_update(app: AppHandle) -> AppResult<Option<UpdateInfo>> {
    if !installed() {
        return Err(msg("Only the installed Hangar updates itself."));
    }
    let found = check(&app).await?;
    if let Some(f) = &found {
        let _ = app.emit("update-available", f);
    }
    Ok(found)
}

/// Downloads the installer (progress goes out as `update-progress`), saving
/// it to a fixed name in the temp folder.
async fn download(app: &AppHandle, http: &reqwest::Client, p: &Pending) -> AppResult<PathBuf> {
    let dir = download_dir();
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("Hangar-setup.exe");

    let mut res = http.get(&p.url).send().await?.error_for_status()?;
    let total = res.content_length().filter(|n| *n > 0).unwrap_or(p.size);
    let mut out = std::fs::File::create(&path)?;
    let mut done: u64 = 0;
    while let Some(chunk) = res.chunk().await? {
        out.write_all(&chunk)?;
        done += chunk.len() as u64;
        let _ = app.emit("update-progress", (done, total));
    }
    out.flush()?;
    drop(out);
    if done != p.size {
        return Err(msg(format!("The download stopped partway ({done} of {} bytes). Try again.", p.size)));
    }
    Ok(path)
}

/// Downloads the update and hands over to its installer: Hangar exits, the
/// installer updates it in place and starts it again. `window` is the one that
/// asked; after the restart Hangar reopens what it showed.
#[tauri::command]
pub async fn install_update(app: AppHandle, window: WebviewWindow) -> AppResult<()> {
    let updates = app.state::<Updates>();
    let pending = updates.pending().clone().ok_or_else(|| msg("There's no update to install. Check again."))?;
    if app.state::<crate::commands::AppState>().busy() {
        return Err(msg("A clone or import is still running in a Hangar window. Let it finish, or cancel it, then update."));
    }
    let setup = download(&app, &updates.http, &pending).await?;

    let showing = app.state::<crate::launch::Windows>().showing_in(window.label());
    let _ = crate::settings::set_last_workspace(&app, showing.as_deref());
    // /P: progress only, no questions (it closes a still-running Hangar itself).
    // /UPDATE: keeps the shortcuts as the user chose them. /R: starts Hangar after.
    std::process::Command::new(&setup)
        .args(["/P", "/UPDATE", "/R"])
        .spawn()
        .map_err(|e| msg(format!("Couldn't start the installer: {e}")))?;
    // Exit at once, like closing the last window, but without letting each
    // window's close overwrite the workspace remembered above.
    app.cleanup_before_exit();
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parses `latest.json` as the release workflow writes it.
    fn latest(version: &str) -> Latest {
        serde_json::from_value(serde_json::json!({
            "version": version,
            "notes": "Notes",
            "pub_date": "2026-11-27T10:00:00Z",
            "url": format!("https://example.test/Hangar_{version}_x64-setup.exe"),
            "size": 42
        }))
        .expect("latest.json")
    }

    #[test]
    fn offers_only_a_newer_release() {
        let p = newer(&latest("0.2.0"), "0.1.0").expect("newer");
        assert_eq!(p.info.version, "0.2.0");
        assert_eq!(p.info.notes.as_deref(), Some("Notes"));
        assert_eq!(p.info.date.as_deref(), Some("2026-11-27T10:00:00Z"));
        assert_eq!(p.url, "https://example.test/Hangar_0.2.0_x64-setup.exe");
        assert_eq!(p.size, 42);

        assert!(newer(&latest("0.1.0"), "0.1.0").is_none(), "same version");
        assert!(newer(&latest("0.0.9"), "0.1.0").is_none(), "older");
        assert!(newer(&latest("0.10.0"), "0.9.0").is_some(), "numeric, not text, order");
        assert!(newer(&latest("v0.2.0"), "0.1.0").is_some(), "a leading v is fine");
        assert!(newer(&latest("soon"), "0.1.0").is_none(), "not a version");
    }

    #[test]
    fn notes_and_date_are_optional() {
        let l: Latest = serde_json::from_str(r#"{"version":"0.2.0","url":"https://example.test/x.exe","size":1}"#).expect("parse");
        let p = newer(&l, "0.1.0").expect("newer");
        assert!(p.info.notes.is_none() && p.info.date.is_none());
    }
}
