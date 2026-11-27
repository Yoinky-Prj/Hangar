import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import {
  CircleCheck,
  FolderInput,
  FolderPlus,
  GitBranch,
  GitFork,
  Info,
  LoaderCircle,
  Lock,
  RefreshCw,
  Search,
  Star,
  X,
} from "lucide-react";
import {
  api,
  errText,
  onJobProgress,
  remoteKey,
  SECTION_LABEL,
  type Account,
  type GhRepo,
  type ImportInspection,
  type JobProgress,
  type ProjectEntry,
  type RepoRef,
  type Section,
  type WorkspaceView,
} from "../api";
import { fmtBytes, newJobId, relTime } from "../util";
import { startJob } from "../jobs";
import { CategoryPicker, JobBar, Modal, SectionPicker, Tabs, useToast } from "./common";

export type AddTab = "github" | "url" | "new" | "import";

/** Text field with a themed suggestion list (branch names of the repo being cloned). */
function BranchInput({ value, onChange, options }: { value: string; onChange: (v: string) => void; options: string[] }) {
  const [open, setOpen] = useState(false);
  const q = value.trim().toLowerCase();
  const matches = options.filter((o) => !q || o.toLowerCase().includes(q)).slice(0, 8);
  return (
    <div className="combo">
      <input
        className="input mono"
        placeholder="default branch"
        value={value}
        onChange={(e) => {
          onChange(e.target.value);
          setOpen(true);
        }}
        onFocus={() => setOpen(true)}
        onBlur={() => setOpen(false)}
        onKeyDown={(e) => e.key === "Escape" && setOpen(false)}
      />
      {open && matches.length > 0 && (
        <div className="combo-list">
          {matches.map((m) => (
            <button
              key={m}
              type="button"
              className={m === value ? "on" : ""}
              onMouseDown={(e) => {
                e.preventDefault();
                onChange(m);
                setOpen(false);
              }}
            >
              <GitBranch size={12} />
              {m}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
type Sec = Exclude<Section, "archive">;

export function AddProjectDialog({
  ws,
  account,
  projects,
  initialTab,
  repoCache,
  setRepoCache,
  onClose,
  onAdded,
  openSettings,
}: {
  ws: WorkspaceView;
  account: Account | null | undefined;
  projects: ProjectEntry[];
  initialTab: AddTab;
  repoCache: GhRepo[] | null;
  setRepoCache: (r: GhRepo[]) => void;
  onClose: () => void;
  onAdded: (p: ProjectEntry) => void;
  openSettings: (tab: "github" | "categories") => void;
}) {
  const toast = useToast();
  const [tab, setTab] = useState<AddTab>(initialTab);
  const [busy, setBusy] = useState(false);
  const [progress, setProgress] = useState<JobProgress | null>(null);
  const jobRef = useRef<string | null>(null);

  const [section, setSection] = useState<Sec>("projects");
  const [cats, setCats] = useState<string[]>([]);
  const [recurse, setRecurse] = useState(true);
  const [folder, setFolder] = useState("");
  const [autoFolder, setAutoFolder] = useState("");
  const [displayName, setDisplayName] = useState("");

  const [repos, setRepos] = useState<GhRepo[] | null>(repoCache);
  const [reposErr, setReposErr] = useState<string | null>(null);
  const [reposLoading, setReposLoading] = useState(false);
  const [searchNonce, setSearchNonce] = useState(0);
  const [filter, setFilter] = useState("");
  const [owner, setOwner] = useState<string>("all");
  const [pick, setPick] = useState<GhRepo | null>(null);

  const [urlInput, setUrlInput] = useState("");
  const [parsed, setParsed] = useState<RepoRef | null>(null);
  const [results, setResults] = useState<GhRepo[] | null>(null);
  const [searching, setSearching] = useState(false);
  const [searchErr, setSearchErr] = useState<string | null>(null);
  const [found, setFound] = useState<GhRepo | null>(null);

  const [branch, setBranch] = useState("");
  const [branchOptions, setBranchOptions] = useState<string[]>([]);
  const [shallow, setShallow] = useState(false);

  const [newName, setNewName] = useState("");
  const [initGit, setInitGit] = useState(true);

  const [imp, setImp] = useState<ImportInspection | null>(null);
  const [impName, setImpName] = useState("");
  const [skipRegen, setSkipRegen] = useState(true);

  const mine = useMemo(
    () => new Set([account?.login, ...(account?.orgs ?? [])].filter((s): s is string => !!s).map((s) => s.toLowerCase())),
    [account],
  );
  const existing = useMemo(() => {
    const m = new Map<string, ProjectEntry>();
    for (const p of projects) {
      const r = p.meta?.source.remote;
      if (r) m.set(remoteKey(r), p);
    }
    return m;
  }, [projects]);

  useEffect(() => {
    const un = onJobProgress((p) => {
      if (p.jobId === jobRef.current) setProgress(p);
    });
    return () => {
      un.then((f) => f());
    };
  }, []);

  /** Reload your repos from GitHub; the current list stays visible meanwhile. */
  const hasRepos = useRef(!!repoCache);
  hasRepos.current = !!repos;
  const loadRepos = useCallback(() => {
    setReposLoading(true);
    api
      .githubListRepos()
      .then((r) => {
        setRepos(r);
        setRepoCache(r);
        setReposErr(null);
      })
      .catch((e) => {
        // Keep showing the list we have; only show the error in place of a missing list.
        if (hasRepos.current) toast("error", `Couldn't refresh your repos: ${errText(e)}`);
        else setReposErr(errText(e));
      })
      .finally(() => setReposLoading(false));
  }, [setRepoCache, toast]);

  // Refresh once each time the dialog is opened (the cached list shows instantly).
  const loadedOnce = useRef(false);
  useEffect(() => {
    if (tab !== "github" || !account || loadedOnce.current) return;
    loadedOnce.current = true;
    loadRepos();
  }, [tab, account, loadRepos]);

  useEffect(() => {
    if (tab !== "url") return;
    setFound(null);
    const t = window.setTimeout(() => {
      api.parseRepo(urlInput).then(setParsed);
    }, 150);
    return () => window.clearTimeout(t);
  }, [urlInput, tab]);

  // Anything that isn't owner/repo or a URL is a GitHub search.
  const query = urlInput.trim();
  useEffect(() => {
    if (tab !== "url" || parsed || query.length < 2) {
      setResults(null);
      setSearchErr(null);
      return;
    }
    let alive = true;
    const t = window.setTimeout(() => {
      setSearching(true);
      api
        .githubSearch(query)
        .then((r) => alive && (setResults(r), setSearchErr(null)))
        .catch((e) => alive && (setResults(null), setSearchErr(errText(e))))
        .finally(() => alive && setSearching(false));
    }, 450);
    return () => {
      alive = false;
      window.clearTimeout(t);
    };
  }, [query, parsed, tab, searchNonce]);

  const target: RepoRef | null =
    tab === "github"
      ? pick
        ? { host: "github.com", owner: pick.owner, repo: pick.name }
        : null
      : tab === "url"
        ? (parsed ?? (found ? { host: "github.com", owner: found.owner, repo: found.name } : null))
        : null;
  const tOwner = target?.owner ?? "";
  const tRepo = target?.repo ?? "";
  const tKey = target ? remoteKey(target) : "";
  const already = tKey ? existing.get(tKey) : undefined;

  // Branch suggestions for the repo being cloned.
  useEffect(() => {
    setBranch("");
    setBranchOptions([]);
    if (!tOwner || target?.host !== "github.com") return;
    let alive = true;
    api
      .githubBranches(tOwner, tRepo)
      .then((b) => alive && setBranchOptions(b))
      .catch(() => {});
    return () => {
      alive = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tOwner, tRepo]);

  useEffect(() => {
    if (!tOwner) {
      setAutoFolder("");
      return;
    }
    // Others' repos default to Reference, and to "latest commit only" (code you keep to read).
    const ownRepo = mine.has(tOwner.toLowerCase());
    setSection(ownRepo ? "projects" : "reference");
    setShallow(!ownRepo);
    api.suggestFolderName(`${tOwner}__${tRepo}`).then(setAutoFolder);
  }, [tOwner, tRepo, mine]);

  useEffect(() => {
    if (tab !== "new") return;
    if (!newName.trim()) {
      setAutoFolder("");
      return;
    }
    api.suggestFolderName(newName).then(setAutoFolder);
  }, [newName, tab]);

  const owners = useMemo(() => {
    if (!repos) return [];
    const set = new Set(repos.map((r) => r.owner));
    const me = account?.login;
    return [...set].sort((a, b) => {
      if (a === me) return -1;
      if (b === me) return 1;
      const am = mine.has(a.toLowerCase()) ? 0 : 1;
      const bm = mine.has(b.toLowerCase()) ? 0 : 1;
      return am - bm || a.localeCompare(b);
    });
  }, [repos, account, mine]);

  const shown = useMemo(() => {
    if (!repos) return [];
    const q = filter.trim().toLowerCase();
    return repos.filter(
      (r) =>
        (owner === "all" || r.owner === owner) &&
        (!q || r.fullName.toLowerCase().includes(q) || (r.description ?? "").toLowerCase().includes(q)),
    );
  }, [repos, owner, filter]);

  function guardedClose() {
    if (!busy) onClose();
  }

  async function run<T>(label: string, fn: (jobId: string) => Promise<T>): Promise<T | null> {
    const jobId = newJobId();
    jobRef.current = jobId;
    setProgress(null);
    setBusy(true);
    // Lets closing the window warn (and offer to cancel cleanly) while this runs.
    const end = startJob(label, () => api.cancelJob(jobId));
    try {
      return await fn(jobId);
    } catch (e) {
      toast("error", errText(e));
      return null;
    } finally {
      end();
      setBusy(false);
      jobRef.current = null;
    }
  }

  async function doClone() {
    if (!target) return;
    const entry = await run(`Cloning ${target.owner}/${target.repo}`, (jobId) =>
      api.cloneRepo(ws.root, jobId, {
        input: `${target.host}/${target.owner}/${target.repo}`,
        section,
        categories: cats,
        recurseSubmodules: recurse,
        folder: folder.trim() || undefined,
        displayName: displayName.trim() || undefined,
        branch: branch.trim() || undefined,
        shallow,
      }),
    );
    if (entry) {
      toast("ok", `Cloned into ${SECTION_LABEL[entry.section]}\\${entry.folder}`);
      onAdded(entry);
    }
  }

  async function doCreate() {
    const entry = await run(`Creating ${newName.trim()}`, () => api.createProject(ws.root, { name: newName, section, initGit, categories: cats }));
    if (entry) {
      toast("ok", `Created ${SECTION_LABEL[entry.section]}\\${entry.folder}`);
      onAdded(entry);
    }
  }

  async function chooseImport() {
    const dir = await open({ directory: true, multiple: false, title: "Choose a folder to import" });
    if (typeof dir !== "string") return;
    try {
      const i = await api.inspectImport(dir);
      setImp(i);
      setImpName(i.suggestedName);
      setSection(i.remote && !mine.has(i.remote.owner.toLowerCase()) ? "reference" : "projects");
    } catch (e) {
      toast("error", errText(e));
    }
  }

  async function doImport() {
    if (!imp) return;
    const res = await run(`Importing ${imp.folder}`, (jobId) =>
      api.importProject(ws.root, jobId, {
        source: imp.path,
        section,
        name: impName,
        displayName: displayName.trim() || undefined,
        categories: cats,
        skipRegenerable: skipRegen,
      }),
    );
    if (res) {
      const { report } = res;
      const skipped = report.skippedDirs.length ? ` Skipped ${report.skippedDirs.length} rebuildable folder(s).` : "";
      const links = report.skippedLinks.length ? ` ${report.skippedLinks.length} link(s) weren't copied.` : "";
      toast("ok", `Imported ${report.files.toLocaleString()} files (${fmtBytes(report.bytes)}).${skipped}${links}`);
      onAdded(res.project);
    }
  }

  const sharedFields = (opts: { clone: boolean; namePlaceholder?: string }) => (
    <>
      {opts.namePlaceholder !== undefined && (
        <div className="field">
          <label>Project name (optional)</label>
          <input className="input" placeholder={opts.namePlaceholder} value={displayName} onChange={(e) => setDisplayName(e.target.value)} />
          <span className="hint">Shown in the list, with {opts.namePlaceholder} under it. Leave empty to show just {opts.namePlaceholder}.</span>
        </div>
      )}
      <div className="row" style={{ gap: 16, alignItems: "flex-start", flexWrap: "wrap" }}>
        <div className="field">
          <label>Put it in</label>
          <SectionPicker value={section} onChange={setSection} />
        </div>
        {opts.clone && (
          <div className="field" style={{ flex: 1, minWidth: 220 }}>
            <label>Folder name</label>
            <input className="input" placeholder={autoFolder || "Owner__Repo"} value={folder} onChange={(e) => setFolder(e.target.value)} />
            <span className="hint">Leave empty to use {autoFolder ? <b className="mono">{autoFolder}</b> : "Owner__Repo"}.</span>
          </div>
        )}
      </div>
      {opts.clone && (
        <div className="row" style={{ gap: 16, alignItems: "flex-start", flexWrap: "wrap" }}>
          <div className="field" style={{ flex: "1 1 260px", maxWidth: 360 }}>
            <label>Branch</label>
            <BranchInput value={branch} onChange={setBranch} options={branchOptions} />
            <span className="hint">Leave empty for the repo's default branch.</span>
          </div>
          <div className="field" style={{ flex: "1 1 260px" }}>
            <label>History</label>
            <div className="segmented">
              <button type="button" className={!shallow ? "on" : ""} onClick={() => setShallow(false)}>
                Full
              </button>
              <button type="button" className={shallow ? "on" : ""} onClick={() => setShallow(true)}>
                Latest commit only
              </button>
            </div>
            <span className="hint">
              {shallow
                ? "Just the newest commit of one branch: much smaller for big repos. You can download the full history later."
                : "Every branch and all of its history."}
            </span>
          </div>
        </div>
      )}
      <div className="field">
        <label>Categories</label>
        <CategoryPicker categories={ws.config.categories} value={cats} onChange={setCats} onManage={() => openSettings("categories")} />
      </div>
      {opts.clone && (
        <label className="check">
          <input type="checkbox" checked={recurse} onChange={(e) => setRecurse(e.target.checked)} />
          Also clone submodules
        </label>
      )}
    </>
  );

  const repoItem = (r: GhRepo, selected: boolean, onPick: () => void) => {
    const inWs = existing.has(remoteKey({ host: "github.com", owner: r.owner, repo: r.name }));
    return (
      <div
        key={r.fullName}
        className={`repo-item ${selected ? "on" : ""} ${inWs ? "disabled" : ""}`}
        onClick={() => !inWs && !busy && onPick()}
      >
        <div className="top">
          <b>{r.fullName}</b>
          {r.private && <Lock size={12} className="muted" />}
          {r.fork && <GitFork size={12} className="muted" />}
          {r.archived && <span className="pill">archived</span>}
          {inWs && (
            <span className="pill ok">
              <CircleCheck size={11} /> in workspace
            </span>
          )}
          <span className="spacer" />
          {r.language && <span className="hint">{r.language}</span>}
          {r.stars > 0 && (
            <span className="hint row" style={{ gap: 3 }}>
              <Star size={11} /> {r.stars.toLocaleString()}
            </span>
          )}
          <span className="hint">{relTime(r.pushedAt)}</span>
        </div>
        {r.description && <div className="desc">{r.description}</div>}
      </div>
    );
  };

  const alreadyNote = already && (
    <div className="callout warn" style={{ marginBottom: 12 }}>
      <Info size={15} />
      <div>
        Already in this workspace as <b>{SECTION_LABEL[already.section]}\{already.folder}</b>.
      </div>
    </div>
  );

  let body: ReactNode;
  let primary: { label: string; onClick: () => void; disabled: boolean };

  if (tab === "github") {
    primary = { label: "Clone", onClick: doClone, disabled: !pick || !!already };
    body =
      account === undefined ? (
        <div className="hint row">
          <LoaderCircle size={14} className="spin" /> Checking GitHub sign-in…
        </div>
      ) : !account ? (
        <div className="callout">
          <Info size={15} />
          <div style={{ flex: 1 }}>
            Sign in to GitHub to pick from your own and your organizations' repos. Public repos can also be cloned from the{" "}
            <b>Search / URL</b> tab without signing in.
            <div style={{ marginTop: 8 }}>
              <button className="btn small primary" onClick={() => openSettings("github")}>
                Sign in to GitHub
              </button>
            </div>
          </div>
        </div>
      ) : (
        <>
          <div className="row">
            <div className="search" style={{ flex: 1, width: "auto" }}>
              <Search size={14} />
              <input className="input" placeholder="Filter repos" value={filter} onChange={(e) => setFilter(e.target.value)} autoFocus />
              {filter && (
                <button className="search-clear" aria-label="Clear filter" onClick={() => setFilter("")}>
                  <X size={13} />
                </button>
              )}
            </div>
            <button className="btn icon" title="Reload your repos from GitHub" onClick={loadRepos} disabled={reposLoading}>
              <RefreshCw size={14} className={reposLoading ? "spin" : ""} />
            </button>
          </div>
          <div className="owners">
            <button className={owner === "all" ? "on" : ""} onClick={() => setOwner("all")}>
              All
            </button>
            {owners.map((o) => (
              <button key={o} className={owner === o ? "on" : ""} onClick={() => setOwner(o)}>
                {o}
              </button>
            ))}
          </div>
          <div className="repo-list">
            {reposErr ? (
              <div className="empty">{reposErr}</div>
            ) : !repos ? (
              <div className="empty">
                <LoaderCircle size={20} className="spin" />
                Loading your repos…
              </div>
            ) : shown.length === 0 ? (
              <div className="empty">No repos match.</div>
            ) : (
              shown.map((r) => repoItem(r, pick?.fullName === r.fullName, () => setPick(r)))
            )}
          </div>
          {pick && <div className="form-panel">{sharedFields({ clone: true, namePlaceholder: pick.fullName })}</div>}
        </>
      );
  } else if (tab === "url") {
    primary = { label: "Clone", onClick: doClone, disabled: !target || !!already };
    body = (
      <>
        <div className="row">
          <div className="search" style={{ flex: 1, width: "auto" }}>
            <Search size={14} />
            <input
              className="input"
              placeholder="Search GitHub, or paste owner/repo or a URL"
              value={urlInput}
              onChange={(e) => setUrlInput(e.target.value)}
              autoFocus
            />
            {urlInput && (
              <button className="search-clear" aria-label="Clear search" onClick={() => setUrlInput("")}>
                <X size={13} />
              </button>
            )}
          </div>
          <button
            className="btn icon"
            title="Search again"
            disabled={!!parsed || query.length < 2 || searching}
            onClick={() => setSearchNonce((n) => n + 1)}
          >
            <RefreshCw size={14} className={searching ? "spin" : ""} />
          </button>
        </div>
        <div className="hint" style={{ margin: "7px 0 10px" }}>
          {parsed ? (
            <span className="row" style={{ gap: 5 }}>
              <CircleCheck size={12} color="var(--ok)" />
              <span>
                <b>
                  {parsed.owner}/{parsed.repo}
                </b>
                {parsed.host !== "github.com" && ` on ${parsed.host}`}
              </span>
            </span>
          ) : query.length >= 2 ? (
            searching ? (
              "Searching GitHub…"
            ) : results ? (
              `${results.length} result${results.length === 1 ? "" : "s"}, most-starred first`
            ) : (
              ""
            )
          ) : (
            "Type a name to search public repos (and your private ones when signed in). SSH addresses are cloned over HTTPS."
          )}
        </div>
        {!parsed && query.length >= 2 && (
          <div className="repo-list">
            {searchErr ? (
              <div className="empty">{searchErr}</div>
            ) : !results ? (
              <div className="empty">
                <LoaderCircle size={20} className="spin" />
                Searching…
              </div>
            ) : results.length === 0 ? (
              <div className="empty">No repos found.</div>
            ) : (
              results.map((r) => repoItem(r, found?.fullName === r.fullName, () => setFound(r)))
            )}
          </div>
        )}
        {alreadyNote}
        {target && <div className="form-panel">{sharedFields({ clone: true, namePlaceholder: `${target.owner}/${target.repo}` })}</div>}
      </>
    );
  } else if (tab === "new") {
    primary = { label: "Create", onClick: doCreate, disabled: !newName.trim() };
    body = (
      <div className="form-panel" style={{ marginTop: 0 }}>
        <div className="field">
          <label>Project name</label>
          <input className="input" value={newName} onChange={(e) => setNewName(e.target.value)} placeholder="my-new-project" autoFocus />
          {autoFolder && autoFolder !== newName.trim() && (
            <span className="hint">
              Folder will be named <b className="mono">{autoFolder}</b>
            </span>
          )}
        </div>
        {sharedFields({ clone: false })}
        <label className="check">
          <input type="checkbox" checked={initGit} onChange={(e) => setInitGit(e.target.checked)} />
          Start a git repository (branch <span className="mono">main</span>)
        </label>
      </div>
    );
  } else {
    primary = { label: "Import", onClick: doImport, disabled: !imp || !impName.trim() };
    body = (
      <>
        <div className="callout" style={{ marginBottom: 14 }}>
          <Info size={15} />
          <div>
            Copies a folder from anywhere on this PC into the workspace, including its <span className="mono">.git</span> history and
            uncommitted files. The original folder is left exactly as it is.
          </div>
        </div>
        <div className="row" style={{ marginBottom: 14 }}>
          <button className="btn" onClick={chooseImport} disabled={busy}>
            <FolderInput size={15} />
            {imp ? "Choose another folder…" : "Choose folder…"}
          </button>
          {imp && (
            <span className="mono selectable" style={{ overflow: "hidden", textOverflow: "ellipsis", color: "var(--text-secondary)" }}>
              {imp.path}
            </span>
          )}
        </div>
        {imp && (
          <>
            <div className="card">
              <div className="kv">
                <span className="k">Git</span>
                <span className="v">{imp.isGit ? "yes" : "not a git repo"}</span>
                <span className="k">Remote</span>
                <span className="v">{imp.remote ? `${imp.remote.owner}/${imp.remote.repo}` : <span className="faint">none</span>}</span>
              </div>
            </div>
            <div className="form-panel">
              {sharedFields({
                clone: false,
                namePlaceholder: imp.remote ? `${imp.remote.owner}/${imp.remote.repo}` : impName || imp.folder,
              })}
              <div className="field">
                <label>Folder name in workspace</label>
                <input className="input" value={impName} onChange={(e) => setImpName(e.target.value)} />
              </div>
              <label className="check">
                <input type="checkbox" checked={skipRegen} onChange={(e) => setSkipRegen(e.target.checked)} />
                Skip rebuildable folders (node_modules, Python virtualenvs, caches, Rust target)
              </label>
              <div className="hint" style={{ marginTop: 2 }}>
                These would break after the move anyway. Reinstall them in the new location (for example <span className="mono">bun install</span>).
              </div>
            </div>
          </>
        )}
      </>
    );
  }

  return (
    <Modal
      title="Add project"
      onClose={guardedClose}
      width={780}
      tall
      locked={busy}
      tabs={
        <Tabs<AddTab>
          value={tab}
          onChange={(t) => {
            setTab(t);
            setFolder("");
            setDisplayName("");
            setCats([]);
          }}
          disabled={busy}
          tabs={[
            { id: "github", label: "From GitHub", icon: <GitBranch size={14} /> },
            { id: "url", label: "Search / URL", icon: <Search size={14} /> },
            { id: "new", label: "New", icon: <FolderPlus size={14} /> },
            { id: "import", label: "Import folder", icon: <FolderInput size={14} /> },
          ]}
        />
      }
      footer={
        busy ? (
          <>
            <JobBar progress={progress} />
            {(tab === "github" || tab === "url" || tab === "import") && (
              <button className="btn" onClick={() => jobRef.current && api.cancelJob(jobRef.current)}>
                Cancel
              </button>
            )}
          </>
        ) : (
          <>
            <span className="hint">
              {tab === "github" || tab === "url"
                ? "Uses Hangar's built-in git. Private repos use your GitHub sign-in."
                : tab === "import"
                  ? "Nothing is deleted or moved from the original folder."
                  : ""}
            </span>
            <div className="spacer" />
            <button className="btn" onClick={onClose}>
              Close
            </button>
            <button className="btn primary" onClick={primary.onClick} disabled={primary.disabled}>
              {primary.label}
            </button>
          </>
        )
      }
    >
      {body}
    </Modal>
  );
}
