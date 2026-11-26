//! Updates for the installed Hangar: it asks GitHub for the latest release at
//! start (and every few hours) and, when asked, downloads that release's
//! installer and runs it in update mode. The standalone exe and dev builds
//! never update themselves.
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

/// The latest published release; drafts and pre-releases never count. A test
/// build can point this at a local server.
const LATEST: &str = match option_env!("HANGAR_LATEST_RELEASE_URL") {
    Some(u) => u,
    None => "https://api.github.com/repos/Yoinky-Prj/Hangar/releases/latest",
};
const RECHECK: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    body: Option<String>,
    published_at: Option<String>,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
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

/// The release, if it's newer than `current` and its installer is attached
/// (the release workflow attaches it some minutes after publishing).
fn newer(release: &Release, current: &str) -> Option<Pending> {
    let version = semver::Version::parse(release.tag_name.trim_start_matches('v')).ok()?;
    if version <= semver::Version::parse(current).ok()? {
        return None;
    }
    let setup = release.assets.iter().find(|a| a.name.ends_with("_x64-setup.exe"))?;
    Some(Pending {
        info: UpdateInfo {
            version: version.to_string(),
            notes: release.body.clone().filter(|b| !b.trim().is_empty()),
            date: release.published_at.clone(),
        },
        url: setup.browser_download_url.clone(),
        size: setup.size,
    })
}

async fn check(app: &AppHandle) -> AppResult<Option<UpdateInfo>> {
    let updates = app.state::<Updates>();
    let res = updates.http.get(LATEST).header("Accept", "application/vnd.github+json").send().await?;
    if res.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(msg("Couldn't check for updates: GitHub has no published release of Hangar (or the repository isn't public)."));
    }
    let release: Release = res.error_for_status()?.json().await?;
    let found = newer(&release, &app.package_info().version.to_string());
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

    fn release(tag: &str, assets: &[&str]) -> Release {
        Release {
            tag_name: tag.into(),
            body: Some("Notes".into()),
            published_at: Some("2026-11-27T10:00:00Z".into()),
            assets: assets
                .iter()
                .map(|n| Asset { name: n.to_string(), browser_download_url: format!("https://example.test/{n}"), size: 42 })
                .collect(),
        }
    }

    #[test]
    fn offers_only_a_newer_release_with_its_installer() {
        let both = ["Hangar_0.2.0_x64-portable.exe", "Hangar_0.2.0_x64-setup.exe"];
        let p = newer(&release("v0.2.0", &both), "0.1.0").expect("newer");
        assert_eq!(p.info.version, "0.2.0");
        assert_eq!(p.url, "https://example.test/Hangar_0.2.0_x64-setup.exe");
        assert_eq!(p.size, 42);

        assert!(newer(&release("v0.1.0", &both), "0.1.0").is_none(), "same version");
        assert!(newer(&release("v0.0.9", &both), "0.1.0").is_none(), "older");
        assert!(newer(&release("0.10.0", &both), "0.9.0").is_some(), "numeric, not text, order; no v");
        assert!(newer(&release("v0.2.0", &["Hangar_0.2.0_x64-portable.exe"]), "0.1.0").is_none(), "installer not attached yet");
        assert!(newer(&release("latest", &both), "0.1.0").is_none(), "tag isn't a version");
    }
}
