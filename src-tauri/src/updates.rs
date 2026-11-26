//! Updates for the installed Hangar: it reads the latest GitHub release's
//! `latest.json` when it starts, whenever it's started again while running,
//! and every hour; when asked, it downloads that release's installer and runs
//! it in update mode. The standalone exe and dev
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
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};

/// `latest.json` of the latest published release; drafts and pre-releases
/// never count. A test build can point this at a local server.
const LATEST: &str = match option_env!("HANGAR_LATEST_RELEASE_URL") {
    Some(u) => u,
    None => "https://github.com/Yoinky-Prj/Hangar/releases/latest/download/latest.json",
};
const RECHECK: Duration = Duration::from_secs(60 * 60);
/// After a failed check (offline, say), try again sooner.
const RETRY: Duration = Duration::from_secs(10 * 60);

/// This build's release notes and publish date. The release workflow sets them
/// at build time, so About can show them offline; a local build has none.
const NOTES: Option<&str> = option_env!("HANGAR_RELEASE_NOTES");
const RELEASED: Option<&str> = option_env!("HANGAR_RELEASE_DATE");

fn non_empty(s: Option<&str>) -> Option<String> {
    s.map(str::trim).filter(|s| !s.is_empty()).map(String::from)
}

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
    /// Wakes the checker before its timer runs out.
    wake: Mutex<Option<mpsc::Sender<()>>>,
}

impl Default for Updates {
    fn default() -> Self {
        Updates { http: crate::github::client(), pending: Mutex::new(None), wake: Mutex::new(None) }
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
    current_notes: Option<String>,
    current_date: Option<String>,
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

/// Checks now, then every hour (or sooner after a failure, or when woken by
/// `check_now`); every window hears about a new version.
pub fn watch(app: &AppHandle) {
    if !installed() {
        return;
    }
    // The installer of the update that just ran.
    let _ = std::fs::remove_dir_all(download_dir());
    let (wake, woken) = mpsc::channel();
    *app.state::<Updates>().wake.lock().unwrap_or_else(|e| e.into_inner()) = Some(wake);
    let app = app.clone();
    std::thread::spawn(move || {
        let mut told: Option<UpdateInfo> = None;
        loop {
            let wait = match tauri::async_runtime::block_on(check(&app)) {
                Ok(found) => {
                    if let Some(found) = found {
                        if told.as_ref() != Some(&found) {
                            let _ = app.emit("update-available", &found);
                            told = Some(found);
                        }
                    }
                    RECHECK
                }
                Err(e) => {
                    eprintln!("{e}");
                    RETRY
                }
            };
            match woken.recv_timeout(wait) {
                Ok(()) | Err(RecvTimeoutError::Timeout) => while woken.try_recv().is_ok() {},
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
    });
}

/// Hangar was started again while it was already running (it keeps one
/// process for all its windows): check now rather than at the next hour.
pub fn check_now(app: &AppHandle) {
    if let Some(wake) = app.state::<Updates>().wake.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        let _ = wake.send(());
    }
}

#[tauri::command]
pub fn update_status(app: AppHandle) -> UpdateStatus {
    UpdateStatus {
        current: app.package_info().version.to_string(),
        current_notes: non_empty(NOTES),
        current_date: non_empty(RELEASED),
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

/// Downloads the update and installs it while Hangar stays open, so the
/// update shows in Hangar's own dialog instead of the installer's window:
/// a running exe can't be overwritten but can be renamed, so this one moves
/// aside, the installer runs silently, and the new version starts once this
/// process has exited. `window` is the one that asked; after the restart
/// Hangar reopens what it showed.
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

    let exe = std::env::current_exe()?;
    let aside = old_exe(&exe);
    let _ = std::fs::remove_file(&aside);
    if std::fs::rename(&exe, &aside).is_err() {
        // Can't move it (no write access to the folder?): let the installer
        // close Hangar and show its own progress window instead.
        std::process::Command::new(&setup)
            .args(["/P", "/UPDATE", "/R"])
            .spawn()
            .map_err(|e| msg(format!("Couldn't start the installer: {e}")))?;
        exit(&app);
    }

    let _ = app.emit("update-installing", ());
    // /S: no window. /UPDATE: keeps the shortcuts as the user chose them.
    // No /R: this process starts the new version itself once the installer
    // is done (the installer can't see the renamed exe, so it leaves this
    // process running); with /R as well, Hangar would start twice.
    let run = {
        let setup = setup.clone();
        tauri::async_runtime::spawn_blocking(move || std::process::Command::new(setup).args(["/S", "/UPDATE"]).status()).await
    };
    let installed_ok = matches!(run, Ok(Ok(ref status)) if status.success()) && exe.is_file();
    if !installed_ok {
        if !exe.is_file() {
            let _ = std::fs::rename(&aside, &exe);
        }
        let why = match run {
            Ok(Ok(status)) => format!("the installer stopped with code {}", status.code().unwrap_or(-1)),
            Ok(Err(e)) => format!("the installer couldn't start: {e}"),
            Err(e) => e.to_string(),
        };
        return Err(msg(format!("The update couldn't be installed ({why}). Hangar is unchanged.")));
    }

    let next = std::process::Command::new(&exe)
        .args(["--after-update", &std::process::id().to_string()])
        .spawn()
        .map_err(|e| msg(format!("The update is installed, but the new version didn't start ({e}). Start Hangar again.")))?;
    // Hangar is in front right now; without this, Windows keeps the new
    // version's window behind whatever gets focus once this one is gone.
    #[cfg(windows)]
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(next.id());
    }
    exit(&app);
}

/// Exits at once, like closing the last window, but without letting each
/// window's close overwrite the workspace remembered for the restart.
fn exit(app: &AppHandle) -> ! {
    app.cleanup_before_exit();
    std::process::exit(0);
}

/// Where the running exe moves during an update.
fn old_exe(exe: &std::path::Path) -> PathBuf {
    exe.with_extension("exe.old")
}

/// Started by the previous version at the end of an update
/// (`--after-update <pid>`): wait for it to exit, since it still holds the
/// single-instance lock, then delete its exe. Call before the app is built.
pub fn finish_update() {
    let args: Vec<String> = std::env::args().collect();
    let Some(i) = args.iter().position(|a| a == "--after-update") else { return };
    if let Some(pid) = args.get(i + 1).and_then(|p| p.parse::<u32>().ok()) {
        wait_for_exit(pid, Duration::from_secs(15));
    }
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(old_exe(&exe));
    }
}

#[cfg(windows)]
fn wait_for_exit(pid: u32, limit: Duration) {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
    unsafe {
        // Already gone if it can't be opened.
        if let Ok(h) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
            WaitForSingleObject(h, limit.as_millis() as u32);
            let _ = CloseHandle(h);
        }
    }
}

#[cfg(not(windows))]
fn wait_for_exit(_pid: u32, _limit: Duration) {}

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
    fn running_exe_moves_next_to_itself() {
        let p = old_exe(std::path::Path::new(r"D:\Apps\Hangar\hangar.exe"));
        assert_eq!(p, std::path::Path::new(r"D:\Apps\Hangar\hangar.exe.old"));
    }

    #[test]
    fn notes_and_date_are_optional() {
        let l: Latest = serde_json::from_str(r#"{"version":"0.2.0","url":"https://example.test/x.exe","size":1}"#).expect("parse");
        let p = newer(&l, "0.1.0").expect("newer");
        assert!(p.info.notes.is_none() && p.info.date.is_none());
    }
}
