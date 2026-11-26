# Hangar

Hangar keeps all your code projects in one place. Point it at a folder and it shows every project with its Git status at a glance, so you can clone, pull, switch branches and open projects in your editor without digging through folders or a terminal.

For Windows 10 and 11 (64-bit). You don't need Git installed.

## Download

Get the latest version from [Releases](https://github.com/Yoinky-Prj/Hangar/releases/latest):

- **Hangar_x.y.z_x64-setup.exe**: the installer (recommended). Hangar then updates itself when a new version comes out.
- **Hangar_x.y.z_x64-portable.exe**: a single file you can run from anywhere. It doesn't update itself; download the new one when you want to update.

The first time you run it, Windows may say "Windows protected your PC" because Hangar isn't code-signed. Click **More info**, then **Run anyway**.

## Getting started

1. Open Hangar and choose a folder to be your workspace, for example `D:\Workspaces`. Hangar sets up `Projects`, `Reference` and `.archive` folders there and leaves everything else alone.
2. Click **Add project** to clone a repository from GitHub (yours, or any public one you search for), start a new project, or import a folder you already have. An imported folder is copied; the original isn't changed.
3. Optional: sign in to GitHub in **Settings › GitHub** to see and clone your own and your organizations' private repositories.

## What you can do

- **See where every project stands**: its branch, uncommitted changes, commits you haven't pushed, and whether it's behind. Projects that need attention are listed on the right.
- **Fetch and pull** one project or all of them at once. Pulling never merges and never overwrites your changes.
- **Work with branches**: switch, create, check out a branch from the remote, update a branch without switching to it, and delete one (Hangar won't delete a branch if you'd lose work).
- **Organize**: tag projects with categories, keep code you only read in Reference, and archive projects you're done with. Unarchive puts them back where they were.
- **Open** a project in File Explorer, VS Code, a terminal, or on GitHub.
- **Use several workspaces**: switch between them from the top bar, or right-click Hangar on the taskbar to open one directly. Each can have its own window.
- **Clone quickly**: download only the latest commit of a big repository, and get its full history later if you need it.
- **Close safely**: if a clone or pull is still running, Hangar asks before closing, and cleans up if you cancel.

## Your files

Your projects stay ordinary folders. Hangar keeps its own notes (categories, display names) in small `.hangar` folders that Git ignores, so they never end up in your commits. Removing a workspace from Hangar, or uninstalling Hangar, leaves your folders where they are.

## Uninstall

Windows Settings › Apps › Hangar › Uninstall. Your workspaces and projects stay on disk.

## Build from source

You need [bun](https://bun.sh) and [Rust](https://rustup.rs).

```powershell
bun install
bun run tauri dev     # run it
bun run tauri build   # build the installer
```
