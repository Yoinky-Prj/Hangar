//! Hangar on the taskbar: its own app ID, and the jump list (right-click the
//! taskbar button): every workspace, which opens it directly, and a "New
//! window" task.
//!
//! Without an explicit app ID, Windows identifies Hangar by its exe path and
//! caches the name and icon it first saw there (the template's icon, from the
//! earliest builds). An explicit ID plus the window's relaunch properties make
//! the taskbar use Hangar's own name and icon instead.

use std::sync::Mutex;

/// Dev builds get their own ID so they never share a taskbar button or jump
/// list with the real app.
pub const APP_ID: &str = if cfg!(debug_assertions) { "io.hangar.desktop.dev" } else { "io.hangar.desktop" };

/// Call before any window exists.
pub fn init_process() {
    #[cfg(windows)]
    unsafe {
        let _ = windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID(&windows::core::HSTRING::from(APP_ID));
    }
}

/// Gives a window Hangar's app ID and what pinning or the jump list's app
/// entry should use: name, icon and command. Call on the main thread before
/// the window is shown.
pub fn tag_window(w: &tauri::WebviewWindow) {
    #[cfg(windows)]
    if let Ok(hwnd) = w.hwnd() {
        if let Err(e) = unsafe { imp::tag(hwnd) } {
            eprintln!("Couldn't set the window's taskbar identity: {e}");
        }
    }
    #[cfg(not(windows))]
    let _ = w;
}

/// Windows keeps the shortcuts that carry Hangar's app ID (the one it made for
/// the jump list's "Hangar" entry, and a taskbar pin) and never updates them.
/// Once hangar.exe moves they point at a missing file: the taskbar button goes
/// blank and the jump list shows whatever icon was cached for the old path.
/// Points those shortcuts at this exe. Call before `tag_window`, so the
/// taskbar never resolves Hangar's ID to the stale shortcut; it only reads a
/// handful of small files.
pub fn repair_shortcuts() {
    #[cfg(windows)]
    imp::repair_shortcuts();
}

/// What was written last, so an unchanged list isn't rewritten. Holding the
/// lock while writing also keeps two updates from overlapping.
static LAST: Mutex<Option<Vec<(String, String)>>> = Mutex::new(None);

/// Rewrites the jump list for these workspace paths (in the background).
pub fn refresh(paths: Vec<String>) {
    #[cfg(windows)]
    std::thread::spawn(move || {
        let items: Vec<(String, String)> = paths
            .iter()
            .map(|p| crate::workspace::summarize(p))
            .filter(|s| s.is_workspace)
            .map(|s| (s.path, s.name))
            .collect();
        let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
        if last.as_ref() == Some(&items) {
            return;
        }
        match imp::write(&items) {
            Ok(()) => *last = Some(items),
            Err(e) => eprintln!("Couldn't update the jump list: {e}"),
        }
    });
    #[cfg(not(windows))]
    let _ = paths;
}

#[cfg(windows)]
mod imp {
    use crate::launch::quote_arg;
    use std::path::{Path, PathBuf};
    use windows::core::{w, Interface, Result, HSTRING, PCWSTR};
    use windows::Win32::Foundation::{HWND, PROPERTYKEY};
    use windows::Win32::Storage::EnhancedStorage::{
        PKEY_AppUserModel_ID, PKEY_AppUserModel_RelaunchCommand, PKEY_AppUserModel_RelaunchDisplayNameResource,
        PKEY_AppUserModel_RelaunchIconResource, PKEY_Title,
    };
    use windows::Win32::System::Com::StructuredStorage::{PropVariantClear, PropVariantToBSTR, PROPVARIANT};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, STGM_READWRITE,
    };
    use windows::Win32::System::Variant::VT_LPWSTR;
    use windows::Win32::UI::Shell::Common::{IObjectArray, IObjectCollection};
    use windows::Win32::UI::Shell::PropertiesSystem::{IPropertyStore, SHGetPropertyStoreForWindow};
    use windows::Win32::UI::Shell::{
        DestinationList, EnumerableObjectCollection, ICustomDestinationList, IShellLinkW, SHChangeNotify, SHStrDupW, ShellLink,
        SHCNE_UPDATEITEM, SHCNF_PATHW,
    };

    fn exe_path() -> Option<String> {
        std::env::current_exe().ok().map(|p| p.to_string_lossy().to_string())
    }

    /// Where Windows keeps taskbar pins, and the shortcuts it makes for apps'
    /// jump lists (ImplicitAppShortcuts).
    fn pinned_dir() -> Option<PathBuf> {
        Some(PathBuf::from(std::env::var_os("APPDATA")?).join(r"Microsoft\Internet Explorer\Quick Launch\User Pinned"))
    }

    pub fn repair_shortcuts() {
        let (Some(exe), Some(dir)) = (exe_path(), pinned_dir()) else { return };
        let links: Vec<PathBuf> = walkdir::WalkDir::new(dir)
            .max_depth(3)
            .into_iter()
            .filter_map(|e| e.ok())
            .map(|e| e.into_path())
            .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("lnk")))
            .collect();
        unsafe {
            let co = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            for p in &links {
                if let Err(e) = repoint(p, &exe) {
                    eprintln!("Couldn't check the shortcut {}: {e}", p.display());
                }
            }
            if co.is_ok() {
                CoUninitialize();
            }
        }
    }

    /// Points the shortcut at `exe` when it carries Hangar's app ID and its
    /// target is gone. Shortcuts to a Hangar that still exists elsewhere (an
    /// installed copy next to a local build) are left alone.
    unsafe fn repoint(path: &Path, exe: &str) -> Result<()> {
        unsafe {
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
            let file: IPersistFile = link.cast()?;
            let at = HSTRING::from(path);
            file.Load(&at, STGM_READWRITE)?;

            let store: IPropertyStore = link.cast()?;
            let mut id = store.GetValue(&PKEY_AppUserModel_ID)?;
            let ours = PropVariantToBSTR(&id).is_ok_and(|b| b.to_string() == super::APP_ID);
            let _ = PropVariantClear(&mut id);
            if !ours {
                return Ok(());
            }

            let mut buf = [0u16; 1024];
            link.GetPath(&mut buf, std::ptr::null_mut(), 0)?;
            let target = String::from_utf16_lossy(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(buf.len())]);
            if target.is_empty() || Path::new(&target).exists() {
                return Ok(());
            }

            link.SetPath(&HSTRING::from(exe))?;
            link.SetIconLocation(&HSTRING::from(exe), 0)?;
            if let Some(folder) = Path::new(exe).parent() {
                link.SetWorkingDirectory(&HSTRING::from(folder))?;
            }
            file.Save(PCWSTR::null(), true)?;
            // So the taskbar drops what it cached for the old target.
            SHChangeNotify(SHCNE_UPDATEITEM, SHCNF_PATHW, Some(at.as_ptr().cast()), None);
            Ok(())
        }
    }

    /// Sets one string property (a CoTaskMem VT_LPWSTR, as the shell expects).
    unsafe fn set_string(store: &IPropertyStore, key: &PROPERTYKEY, value: &str) -> Result<()> {
        unsafe {
            let mut pv = PROPVARIANT::default();
            (*pv.Anonymous.Anonymous).vt = VT_LPWSTR;
            (*pv.Anonymous.Anonymous).Anonymous.pwszVal = SHStrDupW(&HSTRING::from(value))?;
            let r = store.SetValue(key, &pv);
            let _ = PropVariantClear(&mut pv);
            r
        }
    }

    pub unsafe fn tag(hwnd: HWND) -> Result<()> {
        let Some(exe) = exe_path() else { return Ok(()) };
        unsafe {
            let store: IPropertyStore = SHGetPropertyStoreForWindow(hwnd)?;
            set_string(&store, &PKEY_AppUserModel_RelaunchCommand, &quote_arg(&exe))?;
            set_string(&store, &PKEY_AppUserModel_RelaunchDisplayNameResource, "Hangar")?;
            set_string(&store, &PKEY_AppUserModel_RelaunchIconResource, &format!("{exe},0"))?;
            set_string(&store, &PKEY_AppUserModel_ID, super::APP_ID)?;
            store.Commit()
        }
    }

    pub fn write(items: &[(String, String)]) -> Result<()> {
        let Some(exe) = exe_path() else { return Ok(()) };
        unsafe {
            let co = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let r = build(&exe, items);
            if co.is_ok() {
                CoUninitialize();
            }
            r
        }
    }

    /// A shell link to hangar.exe with arguments; the title is what the jump
    /// list shows, the description its tooltip.
    unsafe fn link(exe: &str, args: &str, title: &str, tip: &str, icon: (&str, i32)) -> Result<IShellLinkW> {
        unsafe {
            let l: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
            l.SetPath(&HSTRING::from(exe))?;
            l.SetArguments(&HSTRING::from(args))?;
            l.SetDescription(&HSTRING::from(tip))?;
            l.SetIconLocation(&HSTRING::from(icon.0), icon.1)?;
            let store: IPropertyStore = l.cast()?;
            set_string(&store, &PKEY_Title, title)?;
            store.Commit()?;
            Ok(l)
        }
    }

    /// Arguments of the entries the user removed from the jump list; adding
    /// those back would make the whole category fail.
    unsafe fn removed_arguments(a: &IObjectArray) -> Vec<String> {
        unsafe {
            let n = a.GetCount().unwrap_or(0);
            (0..n)
                .filter_map(|i| {
                    let l: IShellLinkW = a.GetAt(i).ok()?;
                    let mut buf = [0u16; 2048];
                    l.GetArguments(&mut buf).ok()?;
                    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
                    Some(String::from_utf16_lossy(&buf[..end]))
                })
                .collect()
        }
    }

    unsafe fn build(exe: &str, items: &[(String, String)]) -> Result<()> {
        unsafe {
            let list: ICustomDestinationList = CoCreateInstance(&DestinationList, None, CLSCTX_INPROC_SERVER)?;
            list.SetAppID(&HSTRING::from(super::APP_ID))?;
            let mut slots = 0u32;
            let removed: IObjectArray = list.BeginList(&mut slots)?;
            let removed = removed_arguments(&removed);

            let root = std::env::var("SystemRoot").unwrap_or_else(|_| "C:\\Windows".into());
            let folder_icon = format!("{root}\\System32\\shell32.dll");
            let workspaces: IObjectCollection = CoCreateInstance(&EnumerableObjectCollection, None, CLSCTX_INPROC_SERVER)?;
            let mut count = 0;
            for (path, name) in items {
                let args = format!("--workspace {}", quote_arg(path));
                if removed.contains(&args) {
                    continue;
                }
                workspaces.AddObject(&link(exe, &args, name, path, (&folder_icon, 3))?)?;
                count += 1;
            }
            if count > 0 {
                // Fails when "Show recently opened items in Jump Lists" is off
                // in Windows settings; the task below still works then.
                let _ = list.AppendCategory(w!("Workspaces"), &workspaces.cast::<IObjectArray>()?);
            }

            let tasks: IObjectCollection = CoCreateInstance(&EnumerableObjectCollection, None, CLSCTX_INPROC_SERVER)?;
            tasks.AddObject(&link(exe, "--new-window", "New window", "Open a Hangar window on the start screen", (exe, 0))?)?;
            list.AddUserTasks(&tasks.cast::<IObjectArray>()?)?;
            list.CommitList()
        }
    }
}
