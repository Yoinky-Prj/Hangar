//! Opening folders in other apps, and the startup environment check.
//! Git and GitHub are built in; editor and terminal are optional helpers.

use crate::error::{msg, AppResult};
use serde::Serialize;
use std::path::Path;
use std::process::Command;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Tool {
    pub name: String,
    pub found: bool,
    pub path: Option<String>,
    pub required: bool,
    pub note: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Environment {
    pub tools: Vec<Tool>,
    /// False when a required tool is missing; the UI then blocks and explains.
    pub ready: bool,
}

fn find(bin: &str) -> Option<String> {
    which::which(bin).ok().map(|p| p.display().to_string())
}

fn terminal() -> Option<(String, &'static str)> {
    #[cfg(windows)]
    {
        if let Some(p) = find("wt") {
            return Some((p, "Windows Terminal"));
        }
        if let Some(p) = find("pwsh") {
            return Some((p, "PowerShell 7"));
        }
        find("powershell").map(|p| (p, "Windows PowerShell"))
    }
    #[cfg(not(windows))]
    {
        Some(("Terminal".to_string(), "Terminal"))
    }
}

pub fn detect() -> Environment {
    let cred = keyring::Entry::new(crate::github::SERVICE, "probe").is_ok();
    let code = find("code");
    let term = terminal();
    let tools = vec![
        Tool {
            name: "Git engine".into(),
            found: true,
            path: None,
            required: true,
            note: format!("Built in (libgit2 {}). No Git install needed.", crate::git::libgit2_version()),
        },
        Tool {
            name: "Credential store".into(),
            found: cred,
            path: None,
            required: false,
            note: if cred {
                "Keeps your GitHub sign-in in the system credential store.".into()
            } else {
                "The system credential store isn't available, so GitHub sign-in can't be saved.".into()
            },
        },
        Tool {
            name: "VS Code".into(),
            found: code.is_some(),
            path: code,
            required: false,
            note: "Optional. Used by \"Open in VS Code\". Install it from code.visualstudio.com and make sure `code` is on PATH.".into(),
        },
        Tool {
            name: "Terminal".into(),
            found: term.is_some(),
            path: term.as_ref().map(|t| t.0.clone()),
            required: false,
            note: match &term {
                Some((_, kind)) => format!("Optional. \"Open in Terminal\" uses {kind}."),
                None => "Optional. No terminal found for \"Open in Terminal\".".into(),
            },
        },
    ];
    let ready = tools.iter().all(|t| t.found || !t.required);
    Environment { tools, ready }
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct AppIcons {
    /// The installed VS Code's own logo, as a data URL.
    pub vscode: Option<String>,
}

/// VS Code on Windows ships its logo as a PNG for the Start menu tile; the
/// `Code.VisualElementsManifest.xml` next to `Code.exe` says which version
/// folder is current.
fn vscode_logo() -> Option<Vec<u8>> {
    let code = which::which("code").ok()?;
    let root = code.parent()?.parent()?; // ...\Microsoft VS Code\bin\code.cmd
    if let Ok(xml) = std::fs::read_to_string(root.join("Code.VisualElementsManifest.xml")) {
        for key in ["Square150x150Logo=\"", "Square70x70Logo=\""] {
            if let Some(rel) = xml.split(key).nth(1).and_then(|s| s.split('"').next()) {
                if let Ok(bytes) = std::fs::read(root.join(rel)) {
                    return Some(bytes);
                }
            }
        }
    }
    std::fs::read(root.join("resources/app/resources/win32/code_150x150.png")).ok()
}

pub fn app_icons() -> AppIcons {
    use base64::Engine as _;
    AppIcons {
        vscode: vscode_logo()
            .map(|b| format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(b))),
    }
}

#[cfg(windows)]
fn hidden(program: impl AsRef<std::ffi::OsStr>) -> Command {
    use std::os::windows::process::CommandExt;
    let mut c = Command::new(program);
    c.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    c
}

#[cfg(not(windows))]
fn hidden(program: impl AsRef<std::ffi::OsStr>) -> Command {
    Command::new(program)
}

fn require_dir(path: &Path) -> AppResult<()> {
    if path.is_dir() {
        Ok(())
    } else {
        Err(msg(format!("{} doesn't exist any more.", path.display())))
    }
}

pub fn open_in_explorer(path: &Path) -> AppResult<()> {
    require_dir(path)?;
    #[cfg(windows)]
    Command::new("explorer").arg(path).spawn()?;
    #[cfg(target_os = "macos")]
    Command::new("open").arg(path).spawn()?;
    #[cfg(all(unix, not(target_os = "macos")))]
    Command::new("xdg-open").arg(path).spawn()?;
    Ok(())
}

pub fn open_in_vscode(path: &Path) -> AppResult<()> {
    require_dir(path)?;
    let code = which::which("code").map_err(|_| msg("VS Code isn't installed, or `code` isn't on PATH."))?;
    hidden(code).arg(path).spawn()?;
    Ok(())
}

pub fn open_in_terminal(path: &Path) -> AppResult<()> {
    require_dir(path)?;
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        if let Ok(wt) = which::which("wt") {
            Command::new(wt).arg("-d").arg(path).spawn()?;
            return Ok(());
        }
        let shell = which::which("pwsh")
            .or_else(|_| which::which("powershell"))
            .map_err(|_| msg("No terminal found. Install Windows Terminal or PowerShell."))?;
        Command::new(shell).current_dir(path).creation_flags(0x0000_0010).spawn()?; // CREATE_NEW_CONSOLE
    }
    #[cfg(target_os = "macos")]
    Command::new("open").args(["-a", "Terminal"]).arg(path).spawn()?;
    #[cfg(all(unix, not(target_os = "macos")))]
    Command::new("x-terminal-emulator").current_dir(path).spawn()?;
    Ok(())
}
