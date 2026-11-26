import type { KeyboardEvent, MouseEvent, ReactNode } from "react";
import { ArrowDown, ChevronRight, Folder, FolderGit2, Inbox } from "lucide-react";
import type { Category, GitStatus, ProjectEntry, SubmoduleInfo } from "../api";
import { projectSubtitle, projectTitle, relTime, subIssues } from "../util";
import { StatusBadges } from "./common";

export type Sort = "name" | "activity";

/** A visible row: a project, or one of its submodules when expanded. */
export type ListRow = { kind: "project"; p: ProjectEntry } | { kind: "sub"; parent: ProjectEntry; sm: SubmoduleInfo };

export function rowKey(r: ListRow): string {
  return r.kind === "project" ? r.p.path : r.sm.absPath;
}

export function activityOf(p: ProjectEntry, s: GitStatus | null | undefined): number {
  const t = s?.lastCommit?.time ?? p.modified;
  return t ? new Date(t).getTime() : 0;
}

function headLabel(s: GitStatus | null | undefined, commit?: string | null): string {
  if (!s) return "";
  if (s.branch) return ` · ${s.branch}`;
  if (s.detached) return ` · detached${commit ? ` @ ${commit}` : ""}`;
  return "";
}

export function ProjectList({
  projects,
  statuses,
  subs,
  expanded,
  onToggle,
  categories,
  selected,
  onSelect,
  onOpen,
  onContext,
  sort,
  setSort,
  empty,
}: {
  projects: ProjectEntry[];
  statuses: Record<string, GitStatus | null>;
  subs: Record<string, SubmoduleInfo[] | null>;
  expanded: Set<string>;
  onToggle: (path: string, open?: boolean) => void;
  categories: Category[];
  selected: string | null;
  onSelect: (key: string) => void;
  onOpen: (row: ListRow) => void;
  onContext: (e: MouseEvent, row: ListRow) => void;
  sort: Sort;
  setSort: (s: Sort) => void;
  empty: ReactNode;
}) {
  const catMap = new Map(categories.map((c) => [c.id, c]));

  const rows: ListRow[] = [];
  for (const p of projects) {
    rows.push({ kind: "project", p });
    const list = subs[p.path];
    if (expanded.has(p.path) && list) for (const sm of list) rows.push({ kind: "sub", parent: p, sm });
  }

  function hasSubs(p: ProjectEntry) {
    return (statuses[p.path]?.submodules ?? 0) > 0;
  }

  function onKey(e: KeyboardEvent) {
    if (!rows.length) return;
    const i = rows.findIndex((r) => rowKey(r) === selected);
    const cur = i >= 0 ? rows[i] : null;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      onSelect(rowKey(rows[Math.min(rows.length - 1, i + 1)]));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      onSelect(rowKey(rows[Math.max(0, i - 1)]));
    } else if (e.key === "ArrowRight" && cur?.kind === "project" && hasSubs(cur.p)) {
      e.preventDefault();
      onToggle(cur.p.path, true);
    } else if (e.key === "ArrowLeft" && cur) {
      e.preventDefault();
      if (cur.kind === "sub") onSelect(cur.parent.path);
      else onToggle(cur.p.path, false);
    } else if (e.key === "Enter" && cur) {
      onOpen(cur);
    }
  }

  return (
    <>
      <div className="list-head">
        <button onClick={() => setSort("name")}>Name {sort === "name" && <ArrowDown size={12} />}</button>
        <span>Categories</span>
        <span>Status</span>
        <button style={{ justifySelf: "end" }} onClick={() => setSort("activity")}>
          Last activity {sort === "activity" && <ArrowDown size={12} />}
        </button>
      </div>
      <div className="list-body" tabIndex={0} onKeyDown={onKey}>
        {rows.length === 0 ? (
          <div className="empty">
            <Inbox size={34} />
            {empty}
          </div>
        ) : (
          rows.map((row) => {
            const key = rowKey(row);
            const common = {
              className: `list-row ${row.kind === "sub" ? "sub" : ""} ${selected === key ? "on" : ""}`,
              // Hovering a row shows its folder (badges and chips keep their own tooltips).
              title: key,
              onMouseDown: () => onSelect(key),
              onDoubleClick: () => onOpen(row),
              onContextMenu: (e: MouseEvent) => {
                e.preventDefault();
                onSelect(key);
                onContext(e, row);
              },
            };

            if (row.kind === "sub") {
              const { sm } = row;
              const s = sm.status;
              const where = sm.remote ? `${sm.remote.owner}/${sm.remote.repo}` : sm.url ?? "submodule";
              return (
                <div key={key} {...common}>
                  <div className="cell-name">
                    <span className="twisty-space" />
                    <FolderGit2 size={16} className="sub-icon" />
                    <div className="names">
                      <div className="folder">{sm.path}</div>
                      <div className="sub">{where + headLabel(s, sm.checkedOut)}</div>
                    </div>
                  </div>
                  <div className="cell-cats">
                    <span className="pill">submodule</span>
                  </div>
                  <div className="cell-status">
                    {!sm.initialized ? (
                      <span className="pill">not checked out</span>
                    ) : (
                      <>
                        {sm.moved && (
                          <span className="pill violet" title={`Parent records ${sm.recorded}, checked out ${sm.checkedOut}`}>
                            moved
                          </span>
                        )}
                        <StatusBadges isGit status={s} />
                      </>
                    )}
                  </div>
                  <div className="cell-time">{relTime(s?.lastCommit?.time)}</div>
                </div>
              );
            }

            const { p } = row;
            const s = statuses[p.path];
            const cats = (p.meta?.categories ?? []).map((id) => catMap.get(id)).filter((c): c is Category => !!c);
            const when = s?.lastCommit?.time ?? p.modified;
            const open = expanded.has(p.path);
            const sub = projectSubtitle(p);
            return (
              <div key={key} {...common}>
                <div className="cell-name">
                  {hasSubs(p) ? (
                    <button
                      className={`twisty ${open ? "open" : ""}`}
                      title={open ? "Hide submodules" : "Show submodules"}
                      onMouseDown={(e) => e.stopPropagation()}
                      onDoubleClick={(e) => e.stopPropagation()}
                      onClick={() => onToggle(p.path)}
                    >
                      <ChevronRight size={14} />
                    </button>
                  ) : (
                    <span className="twisty-space" />
                  )}
                  {p.isGit ? <FolderGit2 size={18} /> : <Folder size={18} />}
                  <div className="names">
                    <div className="folder">
                      {projectTitle(p)}{" "}
                      {!p.managed && (
                        <span className="pill" title="This folder has no Hangar metadata yet">
                          not managed
                        </span>
                      )}
                    </div>
                    {sub && <div className="sub">{sub}</div>}
                  </div>
                </div>
                <div className="cell-cats">
                  {cats.map((c) => (
                    <span key={c.id} className="cat-chip">
                      <span className="dot" style={{ background: c.color }} />
                      {c.name}
                    </span>
                  ))}
                </div>
                <div className="cell-status">
                  <StatusBadges isGit={p.isGit} status={s} subs={subIssues(subs[p.path])} />
                </div>
                <div className="cell-time" title={when ?? ""}>
                  {relTime(when)}
                </div>
              </div>
            );
          })
        )}
      </div>
    </>
  );
}
