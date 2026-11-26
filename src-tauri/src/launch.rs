//! Windows and launch arguments.
//!
//! `hangar.exe --workspace <path>` opens that workspace, `--new-window` opens
//! a window on the start screen, and a plain start reopens the last workspace.
//! Hangar runs as a single process: a second launch hands its arguments to the
//! running app (which opens or focuses a window) and exits.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use serde::Serialize;
use tauri::{AppHandle, Manager, State, WebviewWindow, WebviewWindowBuilder};

#[derive(Debug, PartialEq)]
pub enum Launch {
    /// Plain start: the last workspace opened, or the start screen.
    Default,
    NewWindow,
    Workspace(String),
}

/// Reads Hangar's own arguments (the program path must already be skipped).
pub fn parse(args: &[String]) -> Launch {
    let mut it = args.iter();
    let mut out = Launch::Default;
    while let Some(a) = it.next() {
        match a.as_str() {
            "--workspace" => {
                if let Some(p) = it.next().filter(|p| !p.trim().is_empty()) {
                    out = Launch::Workspace(p.clone());
                }
            }
            "--new-window" if out == Launch::Default => out = Launch::NewWindow,
            _ => {}
        }
    }
    out
}

/// Quotes one argument for a Windows command line (backslashes only need
/// doubling right before a quote, e.g. a drive root like `D:\`).
pub fn quote_arg(s: &str) -> String {
    let mut out = String::from("\"");
    let mut slashes = 0;
    for c in s.chars() {
        if c == '\\' {
            slashes += 1;
            continue;
        }
        let n = if c == '"' { slashes * 2 + 1 } else { slashes };
        out.extend(std::iter::repeat_n('\\', n));
        out.push(c);
        slashes = 0;
    }
    out.extend(std::iter::repeat_n('\\', slashes * 2));
    out.push('"');
    out
}

/// What a window opens when its UI starts. Neither field set: the last
/// workspace opened (a plain start).
#[derive(Serialize, Clone, Default, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Intent {
    pub workspace: Option<String>,
    pub start_screen: bool,
}

impl Intent {
    pub fn from_launch(l: Launch) -> Intent {
        match l {
            Launch::Default => Intent::default(),
            Launch::NewWindow => Intent { workspace: None, start_screen: true },
            Launch::Workspace(p) => Intent { workspace: Some(p), start_screen: false },
        }
    }
}

#[derive(Default)]
pub struct Windows {
    next: AtomicU32,
    /// What a window should open when its UI starts.
    intents: Mutex<HashMap<String, Intent>>,
    /// The workspace each window shows.
    showing: Mutex<HashMap<String, String>>,
    /// The window focused most recently.
    last_focused: Mutex<Option<String>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Windows {
    pub fn set_intent(&self, label: &str, intent: Intent) {
        if intent != Intent::default() {
            lock(&self.intents).insert(label.to_string(), intent);
        }
    }

    pub fn focused(&self, label: &str) {
        *lock(&self.last_focused) = Some(label.to_string());
    }

    /// The workspace a window shows (None: the start screen).
    pub fn showing_in(&self, label: &str) -> Option<String> {
        lock(&self.showing).get(label).cloned()
    }

    pub fn closed(&self, label: &str) {
        lock(&self.intents).remove(label);
        lock(&self.showing).remove(label);
        let mut last = lock(&self.last_focused);
        if last.as_deref() == Some(label) {
            *last = None;
        }
    }

    fn window_showing(&self, path: &str) -> Option<String> {
        lock(&self.showing).iter().find(|(_, p)| crate::settings::same_path(p, path)).map(|(l, _)| l.clone())
    }
}

/// Shows a window even if its UI never asks to be shown (it starts hidden so
/// it never flashes white).
pub fn show_fallback(w: WebviewWindow) {
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(3));
        if !w.is_visible().unwrap_or(true) {
            let _ = w.show();
        }
    });
}

fn bring_forward(w: &WebviewWindow) {
    let _ = w.unminimize();
    let _ = w.show();
    let _ = w.set_focus();
}

/// A new window like the first one, cascaded by Windows instead of centered.
pub fn open_window(app: &AppHandle, intent: Intent) -> tauri::Result<()> {
    let st = app.state::<Windows>();
    let label = format!("w{}", st.next.fetch_add(1, Ordering::SeqCst) + 1);
    st.set_intent(&label, intent);
    let mut cfg = app.config().app.windows.first().cloned().unwrap_or_default();
    cfg.label = label;
    cfg.center = false;
    cfg.x = None;
    cfg.y = None;
    let w = WebviewWindowBuilder::from_config(app, &cfg)?.build()?;
    let tagged = w.clone();
    let _ = app.run_on_main_thread(move || crate::jumplist::tag_window(&tagged));
    show_fallback(w);
    Ok(())
}

/// A second `hangar.exe` was started while Hangar is running.
pub fn second_launch(app: &AppHandle, argv: Vec<String>) {
    // Starting Hangar again is a natural moment to look for an update.
    crate::updates::check_now(app);
    let launch = parse(argv.get(1..).unwrap_or_default());
    let app = app.clone();
    // Off the callback's thread: building a webview from inside it can stall.
    std::thread::spawn(move || {
        let st = app.state::<Windows>();
        let existing = match &launch {
            Launch::Workspace(p) => st.window_showing(p),
            Launch::Default => lock(&st.last_focused).clone().or_else(|| app.webview_windows().keys().next().cloned()),
            Launch::NewWindow => None,
        };
        if let Some(w) = existing.and_then(|l| app.get_webview_window(&l)) {
            bring_forward(&w);
            return;
        }
        if let Err(e) = open_window(&app, Intent::from_launch(launch)) {
            eprintln!("Couldn't open a window: {e}");
        }
    });
}

/// What this window was opened for. Not consumed: the UI may start twice
/// (dev mode, a reload) and should land in the same place.
#[tauri::command]
pub fn launch_intent(window: WebviewWindow, state: State<'_, Windows>) -> Intent {
    lock(&state.intents).get(window.label()).cloned().unwrap_or_default()
}

/// The UI reports which workspace it shows (None: the start screen), so a
/// jump-list click can focus that window, and the title names it.
#[tauri::command]
pub fn set_window_workspace(window: WebviewWindow, state: State<'_, Windows>, path: Option<String>, name: Option<String>) {
    {
        let mut showing = lock(&state.showing);
        match path {
            Some(p) => showing.insert(window.label().to_string(), p),
            None => showing.remove(window.label()),
        };
    }
    let title = match name {
        Some(n) if !n.trim().is_empty() => format!("{n} \u{2014} Hangar"),
        _ => "Hangar".to_string(),
    };
    let _ = window.set_title(&title);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_arguments() {
        assert_eq!(parse(&args(&[])), Launch::Default);
        assert_eq!(parse(&args(&["--new-window"])), Launch::NewWindow);
        assert_eq!(parse(&args(&["--workspace", "D:\\Work Space"])), Launch::Workspace("D:\\Work Space".into()));
        assert_eq!(parse(&args(&["--workspace"])), Launch::Default);
    }

    #[test]
    fn quotes_paths() {
        assert_eq!(quote_arg("D:\\Workspaces"), "\"D:\\Workspaces\"");
        assert_eq!(quote_arg("D:\\"), "\"D:\\\\\"");
        assert_eq!(quote_arg("C:\\My Work\\x"), "\"C:\\My Work\\x\"");
    }
}
