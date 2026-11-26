import { useRef, useState, type ReactNode } from "react";
import {
  Archive,
  ArrowDownToLine,
  BookOpen,
  Briefcase,
  CircleCheck,
  CloudDownload,
  FolderGit2,
  GitBranch,
  Info,
  LoaderCircle,
  Pencil,
  Undo2,
} from "lucide-react";
import {
  SECTION_LABEL,
  type Category,
  type Environment,
  type GitStatus,
  type ProjectEntry,
  type Section,
  type SourceKind,
  type SubmoduleInfo,
  type WorkspaceView,
} from "../api";
import { fmtDate, hasSubIssues, projectSubtitle, projectTitle, relTime, repoWebUrl, subIssues, uncommitted } from "../util";
import { BranchesSection } from "./Branches";
import { CategoryPicker } from "./common";
import { OpenButtons, type OpenKind } from "./Launchers";

export interface Actions {
  open: (kind: OpenKind, path: string) => void;
  openUrl: (url: string) => void;
  move: (p: ProjectEntry, target: Section) => void;
  unarchive: (p: ProjectEntry) => void;
  adopt: (p: ProjectEntry) => void;
  setCategories: (p: ProjectEntry, ids: string[]) => void;
  copyPath: (path: string) => void;
  /** Set the display name; null clears it (back to owner/repo). */
  rename: (p: ProjectEntry, name: string | null) => void;
  select: (key: string) => void;
  /** Put detached submodules on their branch (`only` = one submodule's folder). */
  attachSubs: (parent: ProjectEntry, only?: string) => void;
  /** Fetch / pull the repo at `path`; `owner` is the project whose status to reload. */
  fetch: (path: string, owner: string) => void;
  pull: (path: string, owner: string) => void;
  /** Turn a "latest commit only" clone into a full one. */
  fullHistory: (path: string, owner: string) => void;
  busyOf: (path: string) => "fetch" | "pull" | "history" | undefined;
  /** Reload a project's git status (and its submodules). */
  reload: (owner: string) => void;
  manageCategories: () => void;
}

const SOURCE_LABEL: Record<SourceKind, string> = {
  clone: "Cloned",
  new: "Created in Hangar",
  import: "Imported",
  adopted: "Adopted",
};

function GitInfo({ isGit, status }: { isGit: boolean; status: GitStatus | null | undefined }) {
  if (!isGit) return <div className="hint">Not a git repository.</div>;
  if (status === undefined)
    return (
      <div className="hint row">
        <LoaderCircle size={14} className="spin" /> Reading repo…
      </div>
    );
  if (status === null) return <div className="callout danger">Couldn't read this repo. It may be damaged or locked.</div>;
  const n = uncommitted(status);
  const plus = status.unpushedCapped ? "+" : "";
  return (
    <>
      <div className="kv">
        <span className="k">Branch</span>
        <span className="v">
          {status.branch ?? (status.detached ? "detached HEAD" : "—")}
          {status.unborn && <span className="faint"> (no commits yet)</span>}
        </span>
        <span className="k">Upstream</span>
        <span className="v">
          {status.upstream ? (
            <>
              {status.upstream}{" "}
              <span className="faint">
                · ↑{status.ahead ?? 0} ↓{status.behind ?? 0}
              </span>
            </>
          ) : (
            <span className="faint">none</span>
          )}
        </span>
        <span className="k">Changes</span>
        <span className="v">
          {n === 0 ? (
            <span className="faint">none</span>
          ) : (
            <>
              {status.staged} staged · {status.modified} modified · {status.untracked} untracked
              {status.conflicted > 0 && <span style={{ color: "var(--danger)" }}> · {status.conflicted} conflicted</span>}
            </>
          )}
        </span>
        <span className="k">Unpushed</span>
        <span className="v">
          {status.unpushed === 0 ? (
            <span className="faint">none</span>
          ) : (
            `${status.unpushed}${plus} commit${status.unpushed === 1 ? "" : "s"} not on any remote`
          )}
        </span>
        <span className="k">Last commit</span>
        <span className="v selectable">
          {status.lastCommit ? (
            <>
              {status.lastCommit.summary}{" "}
              <span className="faint">
                · {status.lastCommit.author} · {relTime(status.lastCommit.time)}
              </span>
            </>
          ) : (
            <span className="faint">—</span>
          )}
        </span>
        <span className="k">Remotes</span>
        <span className="v mono selectable">
          {status.remotes.length ? (
            status.remotes.map((r) => (
              <div key={r.name}>
                {r.name}: {r.url}
              </div>
            ))
          ) : (
            <span className="faint">none</span>
          )}
        </span>
        {status.worktrees > 0 && (
          <>
            <span className="k">Worktrees</span>
            <span className="v">{status.worktrees}</span>
          </>
        )}
      </div>
      {status.unpushed > 0 && status.remotes.length === 0 && (
        <div className="callout warn" style={{ marginTop: 10 }}>
          <Info size={15} />
          <div>This repo has no remote, so its commits exist only on this computer.</div>
        </div>
      )}
    </>
  );
}

/** Project title with a pencil to rename it in place. Only the display name changes. */
function EditableTitle({ project, onRename }: { project: ProjectEntry; onRename: (name: string | null) => void }) {
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState("");
  const cancelled = useRef(false);
  const remote = project.meta?.source.remote;
  const fallback = remote ? `${remote.owner}/${remote.repo}` : project.folder;

  if (editing)
    return (
      <input
        className="input title-input"
        autoFocus
        value={value}
        placeholder={fallback}
        onChange={(e) => setValue(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") e.currentTarget.blur();
          if (e.key === "Escape") {
            cancelled.current = true;
            e.currentTarget.blur();
          }
        }}
        onBlur={() => {
          setEditing(false);
          if (!cancelled.current) onRename(value.trim() || null);
        }}
      />
    );

  return (
    <div className="title-row">
      <h2 className="selectable">{projectTitle(project)}</h2>
      {project.managed && (
        <button
          className="btn ghost icon small"
          title="Rename. Only the name shown in Hangar changes; the folder stays the same."
          onClick={() => {
            cancelled.current = false;
            setValue(project.meta?.name ?? "");
            setEditing(true);
          }}
        >
          <Pencil size={13} />
        </button>
      )}
    </div>
  );
}

/** Why Pull can't run right now, or null when it can. */
export function pullBlocker(s: GitStatus | null | undefined): string | null {
  if (!s) return "Reading repo…";
  if (!s.remotes.length) return "This repo has no remote.";
  if (s.unborn) return "This repo has no commits yet.";
  if (s.detached) return "Detached HEAD: put it on a branch first.";
  if (!s.upstream) return `${s.branch ?? "This branch"} has no upstream branch.`;
  return null;
}

/** "Git" heading with Fetch and Pull buttons, followed by the repo's git details. */
function GitSection({
  path,
  owner,
  isGit,
  status,
  actions,
  children,
}: {
  path: string;
  owner: string;
  isGit: boolean;
  status: GitStatus | null | undefined;
  actions: Actions;
  children?: ReactNode;
}) {
  const busy = actions.busyOf(path);
  const blocker = pullBlocker(status);
  const behind = status?.behind ?? 0;
  return (
    <div className="dsec">
      <div className="dsec-head">
        <h3>Git</h3>
        {isGit && status && status.remotes.length > 0 && (
          <div className="row" style={{ gap: 6 }}>
            <button
              className="btn small"
              disabled={!!busy}
              title="Download new commits from the remote. Your files and branches don't change."
              onClick={() => actions.fetch(path, owner)}
            >
              {busy === "fetch" ? <LoaderCircle size={13} className="spin" /> : <CloudDownload size={13} />}
              Fetch
            </button>
            <button
              className={`btn small ${behind > 0 && !blocker ? "primary" : ""}`}
              disabled={!!busy || !!blocker}
              title={blocker ?? `Fetch, then fast-forward ${status.branch} to ${status.upstream}. Never merges, never overwrites local changes.`}
              onClick={() => actions.pull(path, owner)}
            >
              {busy === "pull" ? <LoaderCircle size={13} className="spin" /> : <ArrowDownToLine size={13} />}
              Pull{behind > 0 ? ` ${behind}` : ""}
            </button>
          </div>
        )}
      </div>
      <GitInfo isGit={isGit} status={status} />
      {status && (status.shallow || status.singleBranch) && (
        <div className="callout" style={{ marginTop: 10 }}>
          <Info size={15} />
          <div style={{ flex: 1 }}>
            {status.shallow && status.singleBranch
              ? "Cloned with the latest commit only: older history and other branches aren't downloaded."
              : status.shallow
                ? "Only recent history is downloaded."
                : "Only one branch is downloaded."}
            <div style={{ marginTop: 8 }}>
              <button className="btn small" disabled={!!busy} onClick={() => actions.fullHistory(path, owner)}>
                {busy === "history" ? <LoaderCircle size={13} className="spin" /> : <CloudDownload size={13} />}
                {busy === "history" ? "Downloading…" : "Download full history"}
              </button>
            </div>
          </div>
        </div>
      )}
      {children}
    </div>
  );
}

function subSummary(sm: SubmoduleInfo): string {
  if (!sm.initialized) return "not checked out";
  const s = sm.status;
  if (!s) return "can't read";
  const parts = [
    s.branch ?? (s.detached ? `detached @ ${sm.checkedOut ?? "?"}` : ""),
    uncommitted(s) ? `${uncommitted(s)} changed` : "",
    s.unpushed ? `${s.unpushed} unpushed` : "",
    sm.moved ? "moved" : "",
  ].filter(Boolean);
  return parts.join(" · ");
}

function SubmoduleList({ list, onSelect }: { list: SubmoduleInfo[] | null | undefined; onSelect: (key: string) => void }) {
  if (list === undefined)
    return (
      <div className="hint row">
        <LoaderCircle size={14} className="spin" /> Reading submodules…
      </div>
    );
  if (list === null) return <div className="hint">Couldn't read the submodules.</div>;
  return (
    <div className="sub-list">
      {list.map((sm) => {
        const dirty = sm.initialized && sm.status && (uncommitted(sm.status) > 0 || sm.status.unpushed > 0);
        return (
          <button key={sm.absPath} className="sub-item" onClick={() => onSelect(sm.absPath)}>
            <FolderGit2 size={15} className="sub-icon" />
            <span className="sub-item-path">{sm.path}</span>
            <span className={`hint ${dirty ? "warn-text" : ""}`}>{subSummary(sm)}</span>
          </button>
        );
      })}
    </div>
  );
}

export function DetailPanel({
  project,
  status,
  subs,
  categories,
  env,
  actions,
}: {
  project: ProjectEntry;
  status: GitStatus | null | undefined;
  subs: SubmoduleInfo[] | null | undefined;
  categories: Category[];
  env: Environment;
  actions: Actions;
}) {
  const meta = project.meta;
  const remote = meta?.source.remote;
  const issues = subIssues(subs);
  const subtitle = projectSubtitle(project);

  return (
    <>
      <EditableTitle key={project.path} project={project} onRename={(n) => actions.rename(project, n)} />
      {subtitle && <div className="title-sub">{subtitle}</div>}
      <div className="path" title="Click to copy the path" onClick={() => actions.copyPath(project.path)}>
        {project.path}
      </div>
      <div className="row" style={{ marginTop: 10, flexWrap: "wrap", gap: 6 }}>
        <span className="pill">{SECTION_LABEL[project.section]}</span>
        {project.section === "archive" && meta?.archivedFrom && <span className="pill">from {SECTION_LABEL[meta.archivedFrom]}</span>}
        {meta && <span className="pill">{SOURCE_LABEL[meta.source.kind]}</span>}
        {!project.managed && <span className="pill warn">Not managed</span>}
      </div>

      {!project.managed && (
        <div className="callout" style={{ marginTop: 12 }}>
          <Info size={15} />
          <div style={{ flex: 1 }}>
            This folder has no Hangar metadata yet (it was probably put here by hand). Adopt it to give it categories.
            <div style={{ marginTop: 8 }}>
              <button className="btn small primary" onClick={() => actions.adopt(project)}>
                <CircleCheck size={13} />
                Adopt
              </button>
            </div>
          </div>
        </div>
      )}

      <div className="dsec">
        <h3>Open</h3>
        <OpenButtons
          path={project.path}
          remote={remote ? { host: remote.host, url: remote.url } : null}
          env={env}
          onOpen={actions.open}
          onOpenUrl={actions.openUrl}
        />
      </div>

      {meta && (
        <div className="dsec">
          <h3>Categories</h3>
          <CategoryPicker
            categories={categories}
            value={meta.categories}
            onChange={(ids) => actions.setCategories(project, ids)}
            onManage={actions.manageCategories}
          />
        </div>
      )}

      <GitSection path={project.path} owner={project.path} isGit={project.isGit} status={status} actions={actions} />
      {project.isGit && <BranchesSection path={project.path} owner={project.path} status={status} reload={actions.reload} />}

      {(status?.submodules ?? 0) > 0 && (
        <div className="dsec">
          <h3 className="row">
            Submodules <span className="faint">{status!.submodules}</span>
          </h3>
          {hasSubIssues(issues) && (
            <div className="callout warn" style={{ marginBottom: 8 }}>
              <Info size={15} />
              <div>Some submodules have work that isn't committed or pushed. It doesn't show in the parent repo's own status.</div>
            </div>
          )}
          <SubmoduleList list={subs} onSelect={actions.select} />
          {subs?.some((s) => s.initialized && s.status?.detached) && (
            <div className="row" style={{ marginTop: 8 }}>
              <button
                className="btn small"
                title="Moves each detached submodule onto its branch (usually main). No files change."
                onClick={() => actions.attachSubs(project)}
              >
                <GitBranch size={13} />
                Put detached submodules on their branch
              </button>
            </div>
          )}
        </div>
      )}

      <div className="dsec">
        <h3>Move</h3>
        <div className="actions">
          {project.section === "archive" ? (
            <>
              <button className="btn" onClick={() => actions.unarchive(project)}>
                <Undo2 size={15} />
                Unarchive
              </button>
              <button className="btn" onClick={() => actions.move(project, project.meta?.archivedFrom === "reference" ? "projects" : "reference")}>
                {project.meta?.archivedFrom === "reference" ? <Briefcase size={15} /> : <BookOpen size={15} />}
                To {project.meta?.archivedFrom === "reference" ? "Projects" : "Reference"}
              </button>
            </>
          ) : (
            <>
              <button className="btn" onClick={() => actions.move(project, project.section === "projects" ? "reference" : "projects")}>
                {project.section === "projects" ? <BookOpen size={15} /> : <Briefcase size={15} />}
                To {project.section === "projects" ? "Reference" : "Projects"}
              </button>
              <button className="btn" onClick={() => actions.move(project, "archive")}>
                <Archive size={15} />
                Archive
              </button>
            </>
          )}
        </div>
      </div>

      {meta && (
        <div className="dsec">
          <h3>About</h3>
          <div className="kv">
            <span className="k">Folder</span>
            <span className="v mono selectable">{project.folder}</span>
            <span className="k">Added</span>
            <span className="v">{fmtDate(meta.createdAt)}</span>
            {meta.source.importedFrom && (
              <>
                <span className="k">Imported from</span>
                <span className="v mono selectable">{meta.source.importedFrom}</span>
              </>
            )}
            {meta.archivedAt && (
              <>
                <span className="k">Archived</span>
                <span className="v">{fmtDate(meta.archivedAt)}</span>
              </>
            )}
            <span className="k">Hangar ID</span>
            <span className="v mono selectable">{meta.id.slice(0, 8)}</span>
          </div>
        </div>
      )}
    </>
  );
}

export function SubmoduleDetail({
  parent,
  sm,
  env,
  actions,
}: {
  parent: ProjectEntry;
  sm: SubmoduleInfo;
  env: Environment;
  actions: Actions;
}) {
  const name = sm.path.split("/").pop() ?? sm.path;
  return (
    <>
      <h2 className="selectable">{name}</h2>
      <div className="path" title="Click to copy the path" onClick={() => actions.copyPath(sm.absPath)}>
        {sm.absPath}
      </div>
      <div className="row" style={{ marginTop: 10, flexWrap: "wrap", gap: 6 }}>
        <span className="pill">Submodule</span>
        <button className="pill pill-link" onClick={() => actions.select(parent.path)} title="Show the parent repo">
          in {projectTitle(parent)}
        </button>
        {sm.moved && <span className="pill violet">moved</span>}
      </div>

      {!sm.initialized && (
        <div className="callout" style={{ marginTop: 12 }}>
          <Info size={15} />
          <div>This submodule isn't checked out, so its folder is empty. Clone the parent again with "Also clone submodules" ticked.</div>
        </div>
      )}

      <div className="dsec">
        <h3>Open</h3>
        <OpenButtons
          path={sm.absPath}
          remote={sm.remote ? { host: sm.remote.host, url: repoWebUrl(sm.remote) } : null}
          env={env}
          onOpen={actions.open}
          onOpenUrl={actions.openUrl}
        />
      </div>

      <div className="dsec">
        <h3>Pinned commit</h3>
        <div className="kv">
          <span className="k">Parent records</span>
          <span className="v mono">{sm.recorded ?? "—"}</span>
          <span className="k">Checked out</span>
          <span className="v mono">{sm.checkedOut ?? "—"}</span>
          <span className="k">URL</span>
          <span className="v mono selectable">{sm.url ?? "—"}</span>
        </div>
        {sm.moved && (
          <div className="callout warn" style={{ marginTop: 10 }}>
            <Info size={15} />
            <div>
              The checked-out commit differs from the one {projectTitle(parent)} records. Commit the submodule change in the parent
              repo to record it, or check out {sm.recorded} again.
            </div>
          </div>
        )}
      </div>

      {sm.initialized && (
        <GitSection path={sm.absPath} owner={parent.path} isGit status={sm.status} actions={actions}>
          {sm.status?.detached && (
            <div className="callout" style={{ marginTop: 10 }}>
              <Info size={15} />
              <div style={{ flex: 1 }}>
                Detached HEAD: you're on a commit, not a branch. Put it on its branch (usually main) to commit and push from here.
                <div style={{ marginTop: 8 }}>
                  <button className="btn small primary" onClick={() => actions.attachSubs(parent, sm.absPath)}>
                    <GitBranch size={13} />
                    Put on its branch
                  </button>
                </div>
              </div>
            </div>
          )}
        </GitSection>
      )}
      {sm.initialized && <BranchesSection path={sm.absPath} owner={parent.path} status={sm.status} reload={actions.reload} />}
    </>
  );
}

export function Summary({
  ws,
  projects,
  statuses,
  subs,
  onSelect,
}: {
  ws: WorkspaceView;
  projects: ProjectEntry[];
  statuses: Record<string, GitStatus | null>;
  subs: Record<string, SubmoduleInfo[] | null>;
  onSelect: (p: ProjectEntry) => void;
}) {
  const by = (s: Section) => projects.filter((p) => p.section === s).length;
  const active = projects.filter((p) => p.section !== "archive");
  const loading = active.some((p) => p.isGit && !(p.path in statuses));
  const attention = active
    .map((p) => ({ p, s: statuses[p.path], si: subIssues(subs[p.path]) }))
    .filter(({ s, si }) => (s && (uncommitted(s) > 0 || s.unpushed > 0)) || hasSubIssues(si))
    .sort((a, b) => (b.s ? b.s.unpushed + uncommitted(b.s) : 0) - (a.s ? a.s.unpushed + uncommitted(a.s) : 0));

  return (
    <>
      <h2>{ws.config.name}</h2>
      <div className="path selectable">{ws.root}</div>
      <div className="dsec">
        <div className="stat-grid">
          <div className="stat">
            <div className="n">{by("projects")}</div>
            <div className="l">Projects</div>
          </div>
          <div className="stat">
            <div className="n">{by("reference")}</div>
            <div className="l">Reference</div>
          </div>
          <div className="stat">
            <div className="n">{by("archive")}</div>
            <div className="l">Archived</div>
          </div>
          <div className="stat">
            <div className="n">{projects.filter((p) => !p.managed).length}</div>
            <div className="l">Not managed yet</div>
          </div>
        </div>
      </div>
      <div className="dsec">
        <h3 className="row">
          Needs attention {loading && <LoaderCircle size={12} className="spin" />}
        </h3>
        {attention.length === 0 ? (
          <div className="hint">{loading ? "Checking repos…" : "Everything active is committed and pushed."}</div>
        ) : (
          <div className="attention">
            {attention.map(({ p, s, si }) => {
              const n = s ? uncommitted(s) : 0;
              const parts = [
                n > 0 ? `${n} changed` : "",
                s && s.unpushed > 0 ? (s.remotes.length ? `${s.unpushed} unpushed` : `${s.unpushed} local only`) : "",
                hasSubIssues(si) ? "submodules" : "",
              ].filter(Boolean);
              return (
                <button key={p.path} onClick={() => onSelect(p)}>
                  <FolderGit2 size={15} color="var(--accent)" />
                  <span style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                    {projectTitle(p)}
                  </span>
                  <span className="hint">{parts.join(" · ")}</span>
                </button>
              );
            })}
          </div>
        )}
      </div>
      <div className="dsec hint">Select a project to see its details. Right-click a project for more actions.</div>
    </>
  );
}
