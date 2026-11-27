import type { ReactNode } from "react";
import { Archive, BookOpen, Briefcase, FolderSearch, Layers, Plus, Settings } from "lucide-react";
import type { Account, Category, ProjectEntry, Section } from "../api";
import { GitHubMark } from "./Launchers";
import type { SettingsTab } from "./SettingsDialog";

export type View =
  | { kind: "all" }
  | { kind: "section"; section: Section }
  | { kind: "category"; id: string }
  | { kind: "unmanaged" };

export function sameView(a: View, b: View): boolean {
  if (a.kind !== b.kind) return false;
  if (a.kind === "section" && b.kind === "section") return a.section === b.section;
  if (a.kind === "category" && b.kind === "category") return a.id === b.id;
  return true;
}

export function viewFilter(v: View): (p: ProjectEntry) => boolean {
  switch (v.kind) {
    case "all":
      return (p) => p.section !== "archive";
    case "section":
      return (p) => p.section === v.section;
    case "category":
      return (p) => p.section !== "archive" && !!p.meta?.categories.includes(v.id);
    case "unmanaged":
      return (p) => !p.managed;
  }
}

export function viewLabel(v: View, categories: Category[]): string {
  switch (v.kind) {
    case "all":
      return "All active";
    case "section":
      return v.section === "projects" ? "Projects" : v.section === "reference" ? "Reference" : "Archive";
    case "category":
      return categories.find((c) => c.id === v.id)?.name ?? "Category";
    case "unmanaged":
      return "Not managed yet";
  }
}

export function Sidebar({
  projects,
  categories,
  view,
  setView,
  account,
  onAddProject,
  onAddCategory,
  onOpenSettings,
}: {
  projects: ProjectEntry[];
  categories: Category[];
  view: View;
  setView: (v: View) => void;
  account: Account | null | undefined;
  onAddProject: () => void;
  onAddCategory: () => void;
  onOpenSettings: (tab: SettingsTab) => void;
}) {
  const count = (v: View) => projects.filter(viewFilter(v)).length;
  const item = (v: View, icon: ReactNode, label: string, tone?: string) => (
    <button className={`side-item ${tone ? `tone-${tone}` : ""} ${sameView(view, v) ? "on" : ""}`} onClick={() => setView(v)}>
      {icon}
      <span>{label}</span>
      <span className="count">{count(v)}</span>
    </button>
  );
  const unmanaged = count({ kind: "unmanaged" });
  return (
    <nav className="sidebar" aria-label="Workspace navigation">
      <div className="side-top">
        <button className="btn primary side-add-btn" onClick={onAddProject} title="Add project (Ctrl+N)">
          <Plus size={15} />
          <span>Add project</span>
          <span className="side-add-kbd">Ctrl+N</span>
        </button>
      </div>

      <div className="side-nav">
        <div className="side-group">
          <div className="side-title">Sections</div>
          {item({ kind: "section", section: "projects" }, <Briefcase size={15} />, "Projects", "projects")}
          {item({ kind: "section", section: "reference" }, <BookOpen size={15} />, "Reference", "reference")}
          {item({ kind: "section", section: "archive" }, <Archive size={15} />, "Archive", "archive")}
          {item({ kind: "all" }, <Layers size={15} />, "All active")}
        </div>

        <div className="side-group">
          <div className="side-title">
            Categories
            <span className="spacer" />
            <button
              className="btn ghost icon small"
              style={{ width: 22, height: 22 }}
              title="New category"
              aria-label="New category"
              onClick={onAddCategory}
            >
              <Plus size={13} />
            </button>
          </div>
          {categories.length === 0 ? (
            <div className="side-empty">Tags you can put on projects, like "Bots" or "Academic".</div>
          ) : (
            categories.map((c) => (
              <div key={c.id}>{item({ kind: "category", id: c.id }, <span className="dot" style={{ background: c.color }} />, c.name)}</div>
            ))
          )}
        </div>

        {unmanaged > 0 && (
          <div className="side-group">
            <div className="side-title">Needs setup</div>
            {item({ kind: "unmanaged" }, <FolderSearch size={15} />, "Not managed yet", "warn")}
          </div>
        )}
      </div>

      <div className="side-foot">
        {account !== undefined && (
          <button
            className="side-account"
            onClick={() => onOpenSettings("github")}
            title={account ? `Signed in to GitHub as ${account.login}` : "Sign in to GitHub in Settings"}
          >
            {account?.avatarUrl ? (
              <img src={account.avatarUrl} alt="" className="side-account-avatar" />
            ) : (
              <GitHubMark size={16} />
            )}
            <span className="side-account-name">{account ? account.login : "Connect GitHub"}</span>
            {account && <span className="side-account-dot" />}
          </button>
        )}
        <button className="side-item" onClick={() => onOpenSettings("github")} title="Settings">
          <Settings size={15} />
          <span>Settings</span>
        </button>
      </div>
    </nav>
  );
}
