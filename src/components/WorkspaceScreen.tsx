import { useCallback, useEffect, useMemo, useRef, useState, type MouseEvent, type ReactNode } from "react";
import {
  Archive,
  ArrowDownToLine,
  BookOpen,
  Briefcase,
  ChevronRight,
  CircleCheck,
  CloudDownload,
  Copy,
  FolderSearch,
  Layers,
  LoaderCircle,
  Plus,
  RefreshCw,
  Search,
  Undo2,
  X,
} from "lucide-react";
import {
  api,
  errText,
  SECTION_LABEL,
  type Account,
  type Environment,
  type GhRepo,
  type GitStatus,
  type ProjectEntry,
  type Section,
  type Settings,
  type SubmoduleInfo,
  type WorkspaceConfig,
  type WorkspaceView,
} from "../api";
import { runningJobs, startJob } from "../jobs";
import { baseName, projectTitle, repoWebUrl } from "../util";
import { AddProjectDialog, type AddTab } from "./AddProjectDialog";
import { HangarMark } from "./Brand";
import { ContextMenu, useToast, type MenuItem } from "./common";
import { DetailPanel, pullBlocker, SubmoduleDetail, Summary, type Actions } from "./DetailPanel";
import { ExplorerIcon, GitHubMark, TerminalIcon, toolFound, useAppIcons, VsCodeIcon } from "./Launchers";
import { activityOf, ProjectList, type ListRow, type Sort } from "./ProjectList";
import { SettingsDialog, type SettingsTab } from "./SettingsDialog";
import { Sidebar, viewFilter, viewLabel, type View } from "./Sidebar";
import { WindowControls } from "./TitleBar";
import { UpdateButton } from "./Update";
import { WorkspaceSetupDialog } from "./WorkspaceSetup";
import { WorkspaceSwitcher } from "./WorkspaceSwitcher";

const STATUS_WORKERS = 4;

function sortByName(list: ProjectEntry[]): ProjectEntry[] {
  return [...list].sort((a, b) => projectTitle(a).localeCompare(projectTitle(b), undefined, { sensitivity: "base" }));
}

export function WorkspaceScreen({
  ws,
  setWs,
  env,
  settings,
  setSettings,
  onLeave,
}: {
  ws: WorkspaceView;
  setWs: (w: WorkspaceView) => void;
  env: Environment;
  settings: Settings;
  setSettings: (s: Settings) => void;
  /** Close this workspace and show the start screen. */
  onLeave: () => void;
}) {
  const toast = useToast();
  const icons = useAppIcons();
  const [projects, setProjects] = useState<ProjectEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [statuses, setStatuses] = useState<Record<string, GitStatus | null>>({});
  const [subs, setSubs] = useState<Record<string, SubmoduleInfo[] | null>>({});
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const [view, setView] = useState<View>({ kind: "section", section: "projects" });
  const [selected, setSelected] = useState<string | null>(null);
  const [search, setSearch] = useState("");
  const [sort, setSort] = useState<Sort>("name");
  const [addTab, setAddTab] = useState<AddTab | null>(null);
  const [settingsTab, setSettingsTab] = useState<SettingsTab | null>(null);
  const [setupPath, setSetupPath] = useState<{ path: string; entryCount: number } | null>(null);
  const [menu, setMenu] = useState<{ x: number; y: number; row: ListRow } | null>(null);
  const [account, setAccount] = useState<Account | null | undefined>(undefined);
  const [accountError, setAccountError] = useState<string | null>(null);
  const [repoCache, setRepoCache] = useState<GhRepo[] | null>(null);
  const [busy, setBusy] = useState<Record<string, "fetch" | "pull" | "history">>({});
  const [bulk, setBulk] = useState<{ kind: "fetch" | "pull"; done: number; total: number } | null>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const statusGen = useRef(0);
  const lastRefresh = useRef(0);

  /** Git status of one repo, plus its submodules when it has any. */
  const loadOne = useCallback(async (path: string, isCurrent: () => boolean = () => true) => {
    let s: GitStatus | null;
    try {
      s = await api.gitStatus(path);
    } catch {
      s = null;
    }
    if (!isCurrent()) return;
    setStatuses((m) => ({ ...m, [path]: s }));
    if (s && s.submodules > 0) {
      let list: SubmoduleInfo[] | null;
      try {
        list = await api.gitSubmodules(path);
      } catch {
        list = null;
      }
      if (isCurrent()) setSubs((m) => ({ ...m, [path]: list }));
    }
  }, []);

  const loadStatuses = useCallback(
    async (list: ProjectEntry[]) => {
      const gen = ++statusGen.current;
      const queue = list.filter((p) => p.isGit).map((p) => p.path);
      const worker = async () => {
        while (queue.length) {
          const path = queue.shift()!;
          await loadOne(path, () => gen === statusGen.current);
          if (gen !== statusGen.current) return;
        }
      };
      await Promise.all(Array.from({ length: STATUS_WORKERS }, worker));
    },
    [loadOne],
  );

  const refresh = useCallback(async () => {
    lastRefresh.current = Date.now();
    setLoading(true);
    try {
      const list = await api.scanProjects(ws.root);
      setProjects(list);
      loadStatuses(list);
    } catch (e) {
      toast("error", errText(e));
    } finally {
      setLoading(false);
    }
  }, [ws.root, loadStatuses, toast]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  // Repos change while you work elsewhere: refresh when the window regains focus.
  useEffect(() => {
    const onFocus = () => {
      if (Date.now() - lastRefresh.current > 10_000) refresh();
    };
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [refresh]);

  useEffect(() => {
    api
      .githubAccount()
      .then((a) => {
        setAccount(a);
        setAccountError(null);
      })
      .catch((e) => {
        setAccount(null);
        setAccountError(errText(e));
      });
  }, []);

  useEffect(() => {
    const dialogOpen = addTab !== null || settingsTab !== null || setupPath !== null;
    const k = (e: KeyboardEvent) => {
      const shortcut =
        ((e.ctrlKey || e.metaKey) && ["f", "n"].includes(e.key.toLowerCase())) || e.key === "F5";
      if (!shortcut) return;
      // A dialog is open (maybe cloning): the app behind it doesn't react.
      // F5 is still swallowed so the page itself never reloads.
      e.preventDefault();
      if (dialogOpen) return;
      if (e.key === "F5") refresh();
      else if (e.key.toLowerCase() === "f") searchRef.current?.focus();
      else setAddTab("github");
    };
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  }, [refresh, addTab, settingsTab, setupPath]);

  /** Swap to another workspace (this screen remounts for it). */
  async function openOther(path: string) {
    try {
      const next = await api.openWorkspace(path);
      setSettings(await api.getSettings());
      setWs(next);
    } catch (e) {
      toast("error", errText(e));
    }
  }

  const categories = ws.config.categories;

  const shown = useMemo(() => {
    const q = search.trim().toLowerCase();
    const catName = new Map(categories.map((c) => [c.id, c.name.toLowerCase()]));
    let list = projects.filter(viewFilter(view));
    if (q) {
      list = list.filter((p) => {
        const r = p.meta?.source.remote;
        const hay = [
          projectTitle(p),
          p.folder,
          r ? `${r.owner}/${r.repo}` : "",
          ...(p.meta?.categories ?? []).map((id) => catName.get(id) ?? ""),
          ...(subs[p.path] ?? []).map((s) => s.path),
        ]
          .join(" ")
          .toLowerCase();
        return hay.includes(q);
      });
    }
    list = sort === "activity" ? [...list].sort((a, b) => activityOf(b, statuses[b.path]) - activityOf(a, statuses[a.path])) : sortByName(list);
    return list;
  }, [projects, view, search, sort, statuses, subs, categories]);

  const selectedProject = projects.find((p) => p.path === selected) ?? null;
  const selectedSub = useMemo(() => {
    if (!selected || selectedProject) return null;
    for (const p of projects) {
      const sm = subs[p.path]?.find((s) => s.absPath === selected);
      if (sm) return { parent: p, sm };
    }
    return null;
  }, [selected, selectedProject, projects, subs]);

  function toggle(path: string, open?: boolean) {
    setExpanded((cur) => {
      const next = new Set(cur);
      const want = open ?? !next.has(path);
      if (want) next.add(path);
      else next.delete(path);
      return next;
    });
  }

  function replace(oldPath: string, entry: ProjectEntry) {
    setProjects((list) => sortByName([...list.filter((p) => p.path !== oldPath), entry]));
    const move = <T,>(m: Record<string, T>) => {
      const next = { ...m };
      if (oldPath !== entry.path && oldPath in next) {
        next[entry.path] = next[oldPath];
        delete next[oldPath];
      }
      return next;
    };
    setStatuses(move);
    setSubs(move);
    setSelected(entry.path);
    if (entry.isGit) loadOne(entry.path);
  }

  function onConfig(cfg: WorkspaceConfig) {
    setWs({ ...ws, config: cfg });
    // Deleting a category edits project metadata too.
    refresh();
  }

  async function guard<T>(fn: () => Promise<T>): Promise<T | undefined> {
    try {
      return await fn();
    } catch (e) {
      toast("error", errText(e));
      return undefined;
    }
  }

  const actions: Actions = {
    open: (kind, path) =>
      guard(() => (kind === "explorer" ? api.openInExplorer(path) : kind === "vscode" ? api.openInVscode(path) : api.openInTerminal(path))),
    openUrl: (url) => guard(() => api.openUrl(url)),
    move: async (p, target) => {
      const e = await guard(() => api.moveProject(ws.root, p.path, target));
      if (e) {
        replace(p.path, e);
        toast("ok", target === "archive" ? `Archived ${projectTitle(p)}.` : `Moved ${projectTitle(p)} to ${SECTION_LABEL[target]}.`);
      }
    },
    unarchive: async (p) => {
      const e = await guard(() => api.unarchiveProject(ws.root, p.path));
      if (e) {
        replace(p.path, e);
        toast("ok", `${projectTitle(p)} is back in ${SECTION_LABEL[e.section]}.`);
      }
    },
    adopt: async (p) => {
      const e = await guard(() => api.adoptProject(ws.root, p.path));
      if (e) replace(p.path, e);
    },
    setCategories: async (p, ids) => {
      const e = await guard(() => api.setProjectCategories(ws.root, p.path, ids));
      if (e) setProjects((list) => list.map((x) => (x.path === p.path ? e : x)));
    },
    copyPath: (path) => {
      navigator.clipboard.writeText(path).then(() => toast("info", "Path copied."));
    },
    rename: async (p, name) => {
      if ((p.meta?.name?.trim() || null) === name) return;
      const e = await guard(() => api.setProjectName(ws.root, p.path, name));
      if (e) setProjects((list) => list.map((x) => (x.path === p.path ? e : x)));
    },
    select: (key) => {
      // Selecting a submodule shows it in the list too.
      for (const p of projects) {
        if (subs[p.path]?.some((s) => s.absPath === key)) toggle(p.path, true);
      }
      setSelected(key);
    },
    attachSubs: async (parent, only) => {
      const res = await guard(() => api.attachSubmodules(parent.path, only));
      if (!res) return;
      const moved = res.filter((r) => r.branch && r.note === null);
      const behind = res.filter((r) => r.branch && r.note);
      const left = res.filter((r) => !r.branch);
      const parts = [
        moved.length || behind.length ? `${moved.length + behind.length} submodule(s) on their branch.` : "",
        ...behind.map((r) => `${r.path}: ${r.note}`),
        ...left.map((r) => `${r.path}: ${r.note}`),
      ].filter(Boolean);
      toast(left.length ? "error" : "ok", parts.join(" ") || "Nothing to change.");
      loadOne(parent.path);
    },
    fetch: (path, owner) =>
      runGit(path, owner, "fetch", async () => {
        await api.gitFetch(path);
        const s = await api.gitStatus(path).catch(() => null);
        const name = baseName(path);
        if (!s?.upstream) return `Fetched ${name}.`;
        return s.behind
          ? `Fetched ${name}: ${s.behind} new commit${s.behind === 1 ? "" : "s"} on ${s.upstream} to pull.`
          : `Fetched ${name}: up to date with ${s.upstream}.`;
      }),
    pull: (path, owner) =>
      runGit(path, owner, "pull", async () => {
        const out = await api.gitPull(path);
        const name = baseName(path);
        let text =
          out.commits === 0
            ? `${name} is already up to date with ${out.upstream}.`
            : `Pulled ${out.commits} commit${out.commits === 1 ? "" : "s"} into ${name} (${out.branch}).`;
        if (out.movedSubmodules.length)
          text += ` These submodules are now pinned to a different commit: ${out.movedSubmodules.join(", ")}.`;
        return text;
      }),
    fullHistory: (path, owner) =>
      runGit(path, owner, "history", async () => {
        await api.gitFullHistory(path);
        return `Downloaded the full history and all branches of ${baseName(path)}.`;
      }),
    busyOf: (path) => busy[path],
    reload: (owner) => {
      loadOne(owner);
    },
    manageCategories: () => setSettingsTab("categories"),
  };

  /** Run a fetch/pull with a per-repo busy flag, report it, then reload the owning project. */
  async function runGit(path: string, owner: string, kind: "fetch" | "pull" | "history", fn: () => Promise<string>) {
    if (busy[path]) return;
    setBusy((b) => ({ ...b, [path]: kind }));
    const verb = kind === "fetch" ? "Fetching" : kind === "pull" ? "Pulling" : "Downloading the full history of";
    const end = startJob(`${verb} ${baseName(path)}`);
    try {
      toast("ok", await fn());
    } catch (e) {
      toast("error", errText(e));
    } finally {
      end();
      setBusy((b) => {
        const next = { ...b };
        delete next[path];
        return next;
      });
      loadOne(owner);
    }
  }

  /**
   * Fetch or pull every active repo with a remote, and their checked-out
   * submodules, 4 at a time. Archived projects are skipped. Pull is the same
   * fast-forward-only pull as the per-repo button.
   */
  async function runAll(kind: "fetch" | "pull") {
    if (bulk) return;
    const targets: { path: string; owner: string; label: string; isSub: boolean }[] = [];
    let skipped = 0;
    const consider = (path: string, owner: string, label: string, isSub: boolean, s: GitStatus | null | undefined) => {
      if (s && s.remotes.length === 0) return;
      if (kind === "pull" && s && pullBlocker(s)) {
        skipped++;
        return;
      }
      targets.push({ path, owner, label, isSub });
    };
    for (const p of projects) {
      if (p.section === "archive" || !p.isGit) continue;
      consider(p.path, p.path, projectTitle(p), false, statuses[p.path]);
      for (const sm of subs[p.path] ?? []) {
        if (sm.initialized && sm.status) consider(sm.absPath, p.path, `${projectTitle(p)} › ${sm.path}`, true, sm.status);
      }
    }
    if (!targets.length) {
      toast("info", kind === "fetch" ? "No active repos with a remote to fetch." : "No active repos can be pulled (they need a branch with an upstream).");
      return;
    }

    setBulk({ kind, done: 0, total: targets.length });
    setBusy((b) => ({ ...b, ...Object.fromEntries(targets.map((t) => [t.path, kind])) }));
    const endJob = startJob(kind === "fetch" ? "Fetching all repos" : "Pulling all repos");
    const changed: string[] = [];
    const failed: string[] = [];
    const moved: string[] = [];
    let subsAdvanced = 0;
    let ok = 0;
    const queue = [...targets];
    const worker = async () => {
      while (queue.length) {
        const t = queue.shift()!;
        try {
          if (kind === "fetch") {
            await api.gitFetch(t.path);
            ok++;
            const s = await api.gitStatus(t.path).catch(() => null);
            if (s?.behind) changed.push(`${t.label} (${s.behind})`);
          } else {
            const out = await api.gitPull(t.path);
            ok++;
            if (out.commits) {
              changed.push(`${t.label} (+${out.commits})`);
              if (t.isSub) subsAdvanced++;
            }
            for (const m of out.movedSubmodules) moved.push(`${t.label} › ${m}`);
          }
        } catch (e) {
          const m = errText(e);
          if (!m.includes("no remote")) failed.push(`${t.label}: ${m}`);
        } finally {
          setBusy((b) => {
            const next = { ...b };
            delete next[t.path];
            return next;
          });
          setBulk((f) => f && { ...f, done: f.done + 1 });
        }
      }
    };
    try {
      await Promise.all(Array.from({ length: STATUS_WORKERS }, worker));
    } finally {
      endJob();
      setBulk(null);
    }
    await Promise.all([...new Set(targets.map((t) => t.owner))].map((o) => loadOne(o)));

    const list = (xs: string[]) => (xs.length > 5 ? `${xs.slice(0, 5).join(", ")} and ${xs.length - 5} more` : xs.join(", "));
    const s = (n: number) => (n === 1 ? "" : "s");
    const parts =
      kind === "fetch"
        ? [
            `Fetched ${ok} repo${s(ok)}.`,
            changed.length ? `New commits to pull: ${list(changed)}.` : "Everything is up to date.",
          ]
        : [
            changed.length
              ? `Pulled new commits into ${changed.length} repo${s(changed.length)}: ${list(changed)}.`
              : `All ${ok} repo${s(ok)} were already up to date.`,
            skipped ? `Skipped ${skipped} (detached or no upstream).` : "",
            subsAdvanced
              ? `${subsAdvanced} submodule${s(subsAdvanced)} moved past the commit their parent pins; commit the parent to record it.`
              : "",
            moved.length ? `Pulled parents now pin different submodule commits: ${list(moved)}.` : "",
          ];
    if (failed.length) parts.push(`${failed.length} couldn't ${kind}: ${list(failed)}`);
    toast(failed.length ? "error" : "ok", parts.filter(Boolean).join(" "));
  }

  /** Tooltip for the current view in the breadcrumb: its folder when it is one. */
  function viewTitle(v: View): string {
    const sep = ws.root.includes("\\") ? "\\" : "/";
    const dir = (name: string) => ws.root.replace(/[\\/]+$/, "") + sep + name;
    switch (v.kind) {
      case "section":
        return dir(v.section === "projects" ? "Projects" : v.section === "reference" ? "Reference" : ".archive");
      case "all":
        return `Projects and Reference in ${ws.root}`;
      case "category":
        return "A category: a tag on projects, not a folder";
      case "unmanaged":
        return "Folders in this workspace without Hangar metadata";
    }
  }

  function gitItems(path: string, owner: string, s: GitStatus | null | undefined): MenuItem[] {
    if (!s || !s.remotes.length) return [];
    const blocker = pullBlocker(s);
    return [
      { label: "Fetch", icon: <CloudDownload size={14} />, onClick: () => actions.fetch(path, owner), disabled: !!busy[path] },
      {
        label: s.behind ? `Pull ${s.behind} commit${s.behind === 1 ? "" : "s"}` : "Pull",
        icon: <ArrowDownToLine size={14} />,
        onClick: () => actions.pull(path, owner),
        disabled: !!busy[path] || !!blocker,
      },
      "sep",
    ];
  }

  function openRow(row: ListRow) {
    const path = row.kind === "project" ? row.p.path : row.sm.absPath;
    actions.open(toolFound(env, "VS Code") ? "vscode" : "explorer", path);
  }

  function openItems(path: string, webUrl: string | null, host: string | null): MenuItem[] {
    const items: MenuItem[] = [
      { label: "Open in Explorer", icon: <ExplorerIcon size={16} />, onClick: () => actions.open("explorer", path) },
      {
        label: "Open in VS Code",
        icon: <VsCodeIcon size={16} src={icons.vscode} />,
        onClick: () => actions.open("vscode", path),
        disabled: !toolFound(env, "VS Code"),
      },
      {
        label: "Open in Terminal",
        icon: <TerminalIcon size={16} />,
        onClick: () => actions.open("terminal", path),
        disabled: !toolFound(env, "Terminal"),
      },
    ];
    if (webUrl)
      items.push({
        label: `Open on ${host === "github.com" ? "GitHub" : host}`,
        icon: <GitHubMark size={15} />,
        onClick: () => actions.openUrl(webUrl),
      });
    return items;
  }

  function menuItems(row: ListRow): MenuItem[] {
    if (row.kind === "sub") {
      const { sm, parent } = row;
      return [
        ...openItems(sm.absPath, sm.remote ? repoWebUrl(sm.remote) : null, sm.remote?.host ?? null),
        "sep",
        ...gitItems(sm.absPath, parent.path, sm.status),
        { label: `Show ${projectTitle(parent)}`, icon: <ChevronRight size={14} />, onClick: () => setSelected(parent.path) },
        { label: "Copy path", icon: <Copy size={14} />, onClick: () => actions.copyPath(sm.absPath) },
      ];
    }
    const p = row.p;
    const remote = p.meta?.source.remote;
    const items: MenuItem[] = [
      ...openItems(p.path, remote?.url ?? null, remote?.host ?? null),
      "sep",
      ...(p.isGit ? gitItems(p.path, p.path, statuses[p.path]) : []),
    ];
    const moves: [Section, string, ReactNode][] = [
      ["projects", "Move to Projects", <Briefcase size={14} />],
      ["reference", "Move to Reference", <BookOpen size={14} />],
    ];
    if (p.section === "archive") {
      items.push({ label: "Unarchive", icon: <Undo2 size={14} />, onClick: () => actions.unarchive(p) });
    }
    for (const [s, label, icon] of moves) if (s !== p.section) items.push({ label, icon, onClick: () => actions.move(p, s) });
    if (p.section !== "archive") items.push({ label: "Archive", icon: <Archive size={14} />, onClick: () => actions.move(p, "archive") });
    items.push("sep");
    if (!p.managed) items.push({ label: "Adopt into Hangar", icon: <CircleCheck size={14} />, onClick: () => actions.adopt(p) });
    items.push({ label: "Copy path", icon: <Copy size={14} />, onClick: () => actions.copyPath(p.path) });
    return items;
  }

  const emptyText =
    search.trim() ? (
      <div>No projects match “{search.trim()}”.</div>
    ) : view.kind === "section" && view.section === "archive" ? (
      <div>Nothing archived yet. Archive a project from its right-click menu.</div>
    ) : view.kind === "category" ? (
      <div>No active projects in this category. Add it from a project's detail panel.</div>
    ) : (
      <>
        <div>No projects here yet.</div>
        <button className="btn primary" onClick={() => setAddTab("github")}>
          <Plus size={14} />
          Add project
        </button>
      </>
    );

  const totalInView = useMemo(() => projects.filter(viewFilter(view)).length, [projects, view]);

  const viewBadge = (() => {
    switch (view.kind) {
      case "section":
        if (view.section === "projects") return { icon: <Briefcase size={14} />, tone: "projects" };
        if (view.section === "reference") return { icon: <BookOpen size={14} />, tone: "reference" };
        return { icon: <Archive size={14} />, tone: "archive" };
      case "all":
        return { icon: <Layers size={14} />, tone: "" };
      case "unmanaged":
        return { icon: <FolderSearch size={14} />, tone: "warn" };
      case "category": {
        const cat = categories.find((c) => c.id === view.id);
        return { icon: <span className="dot" style={{ background: cat?.color ?? "var(--accent)" }} />, tone: "" };
      }
    }
  })();

  return (
    <div className="shell">
      <header className="topbar" data-tauri-drag-region>
        <div className="topbar-left" data-tauri-drag-region>
          <div className="brand drag-through" data-tauri-drag-region>
            <div className="logo">
              <HangarMark size={15} />
            </div>
            Hangar
          </div>
          <span className="topbar-slash">/</span>
          <WorkspaceSwitcher
            ws={ws}
            setSettings={setSettings}
            onOpen={openOther}
            onSetup={(path, entryCount) => setSetupPath({ path, entryCount })}
          />
          <div className="spacer" data-tauri-drag-region />
        </div>

        <div className="topbar-center">
          <div className={`search ${!search ? "has-kbd" : ""}`}>
            <Search size={14} />
            <input
              ref={searchRef}
              className="input"
              placeholder="Search projects…"
              aria-label="Search projects"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Escape") {
                  if (search) setSearch("");
                  else e.currentTarget.blur();
                }
              }}
            />
            {search ? (
              <button
                className="search-clear"
                aria-label="Clear search"
                title="Clear search"
                onClick={() => {
                  setSearch("");
                  searchRef.current?.focus();
                }}
              >
                <X size={13} />
              </button>
            ) : (
              <kbd className="kbd-hint">Ctrl+F</kbd>
            )}
          </div>
        </div>

        <div className="topbar-right" data-tauri-drag-region>
          <div className="spacer" data-tauri-drag-region />
          <UpdateButton />
          <WindowControls />
        </div>
      </header>

      <Sidebar
        projects={projects}
        categories={categories}
        view={view}
        setView={(v) => {
          setView(v);
          setSelected(null);
        }}
        account={account}
        onAddProject={() => setAddTab("github")}
        onAddCategory={() => setSettingsTab("categories")}
        onOpenSettings={(tab) => setSettingsTab(tab)}
      />

      <main className="main">
        <div className="view-bar">
          <div className="view-bar-title">
            <span className={`view-bar-icon ${viewBadge.tone ? `tone-${viewBadge.tone}` : ""}`}>{viewBadge.icon}</span>
            <h1 title={viewTitle(view)}>{viewLabel(view, categories)}</h1>
            <span className="view-bar-count" title={`${shown.length} shown`}>
              {search.trim() ? `${shown.length} / ${totalInView}` : shown.length}
            </span>
          </div>
          <div className="spacer" />
          <div className="view-bar-actions">
            <button className="btn icon" title="Refresh (F5)" aria-label="Refresh" onClick={refresh}>
              <RefreshCw size={14} className={loading ? "spin" : ""} />
            </button>
            <div className="btn-group">
              <button
                className={`btn bulk ${bulk?.kind === "fetch" ? "running" : ""}`}
                onClick={() => runAll("fetch")}
                disabled={!!bulk}
                title="Fetch every repo and submodule that has a remote (archived ones are skipped). Files and branches don't change."
              >
                {bulk?.kind === "fetch" ? <LoaderCircle size={14} className="spin" /> : <CloudDownload size={14} />}
                <span className="bulk-label">{bulk?.kind === "fetch" ? `Fetching ${bulk.done}/${bulk.total}` : "Fetch all"}</span>
              </button>
              <button
                className={`btn bulk ${bulk?.kind === "pull" ? "running" : ""}`}
                onClick={() => runAll("pull")}
                disabled={!!bulk}
                title="Fast-forward every repo and submodule that's on a branch with an upstream (archived ones are skipped). Never merges, never overwrites local changes."
              >
                {bulk?.kind === "pull" ? <LoaderCircle size={14} className="spin" /> : <ArrowDownToLine size={14} />}
                <span className="bulk-label">{bulk?.kind === "pull" ? `Pulling ${bulk.done}/${bulk.total}` : "Pull all"}</span>
              </button>
            </div>
          </div>
        </div>
        <ProjectList
          projects={shown}
          statuses={statuses}
          subs={subs}
          expanded={expanded}
          onToggle={toggle}
          categories={categories}
          selected={selected}
          onSelect={setSelected}
          onOpen={openRow}
          onContext={(e: MouseEvent, row) => setMenu({ x: e.clientX, y: e.clientY, row })}
          sort={sort}
          setSort={setSort}
          empty={emptyText}
        />
      </main>

      <aside className="detail">
        {selectedProject ? (
          <DetailPanel
            project={selectedProject}
            status={statuses[selectedProject.path]}
            subs={subs[selectedProject.path]}
            categories={categories}
            env={env}
            actions={actions}
            onClose={() => setSelected(null)}
          />
        ) : selectedSub ? (
          <SubmoduleDetail parent={selectedSub.parent} sm={selectedSub.sm} env={env} actions={actions} onClose={() => setSelected(null)} />
        ) : (
          <Summary
            ws={ws}
            projects={projects}
            statuses={statuses}
            subs={subs}
            onSelect={(p) => {
              if (!viewFilter(view)(p)) setView({ kind: "section", section: p.section });
              setSelected(p.path);
            }}
            onCopyPath={actions.copyPath}
            onNavigate={(v) => {
              setView(v);
              setSelected(null);
            }}
          />
        )}
      </aside>

      {menu && <ContextMenu x={menu.x} y={menu.y} items={menuItems(menu.row)} onClose={() => setMenu(null)} />}

      {addTab && (
        <AddProjectDialog
          ws={ws}
          account={account}
          projects={projects}
          initialTab={addTab}
          repoCache={repoCache}
          setRepoCache={setRepoCache}
          onClose={() => setAddTab(null)}
          onAdded={(p) => {
            setAddTab(null);
            setView({ kind: "section", section: p.section });
            replace(p.path, p);
          }}
          openSettings={(t) => {
            setAddTab(null);
            setSettingsTab(t);
          }}
        />
      )}

      {settingsTab && (
        <SettingsDialog
          initialTab={settingsTab}
          ws={ws}
          projects={projects}
          settings={settings}
          setSettings={setSettings}
          env={env}
          account={account}
          accountError={accountError}
          setAccount={(a) => {
            setAccount(a);
            setAccountError(null);
            setRepoCache(null);
          }}
          onConfig={onConfig}
          onRemove={async () => {
            const jobs = runningJobs();
            if (jobs.length) {
              toast("error", `Wait for "${jobs[0].label}" to finish before removing this workspace.`);
              return;
            }
            try {
              setSettings(await api.forgetWorkspace(ws.root));
            } catch (e) {
              toast("error", errText(e));
              return;
            }
            toast("ok", `Removed "${ws.config.name}" from Hangar. Its folder is untouched.`);
            setSettingsTab(null);
            onLeave();
          }}
          onClose={() => setSettingsTab(null)}
        />
      )}

      {setupPath && (
        <WorkspaceSetupDialog
          path={setupPath.path}
          entryCount={setupPath.entryCount}
          onClose={() => setSetupPath(null)}
          onCreated={async (next) => {
            setSettings(await api.getSettings());
            setWs(next);
          }}
        />
      )}
    </div>
  );
}
