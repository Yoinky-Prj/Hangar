import type { ReactNode } from "react";
import { Archive, BookOpen, Briefcase, FolderSearch, Layers, Plus } from "lucide-react";
import type { Category, ProjectEntry, Section } from "../api";

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
  onAddCategory,
}: {
  projects: ProjectEntry[];
  categories: Category[];
  view: View;
  setView: (v: View) => void;
  onAddCategory: () => void;
}) {
  const count = (v: View) => projects.filter(viewFilter(v)).length;
  const item = (v: View, icon: ReactNode, label: string) => (
    <button className={`side-item ${sameView(view, v) ? "on" : ""}`} onClick={() => setView(v)}>
      {icon}
      <span>{label}</span>
      <span className="count">{count(v)}</span>
    </button>
  );
  const unmanaged = count({ kind: "unmanaged" });
  return (
    <nav className="sidebar">
      <div className="side-title">Sections</div>
      {item({ kind: "section", section: "projects" }, <Briefcase size={15} />, "Projects")}
      {item({ kind: "section", section: "reference" }, <BookOpen size={15} />, "Reference")}
      {item({ kind: "section", section: "archive" }, <Archive size={15} />, "Archive")}
      {item({ kind: "all" }, <Layers size={15} />, "All active")}

      <div className="side-title">
        Categories
        <span className="spacer" />
        <button className="btn ghost icon small" style={{ width: 22, height: 22, color: "var(--muted)" }} title="New category" onClick={onAddCategory}>
          <Plus size={13} />
        </button>
      </div>
      {categories.length === 0 && (
        <div className="hint" style={{ padding: "0 8px" }}>
          Tags you can put on projects, like "Bots" or "Academic".
        </div>
      )}
      {categories.map((c) => (
        <div key={c.id}>{item({ kind: "category", id: c.id }, <span className="dot" style={{ background: c.color }} />, c.name)}</div>
      ))}

      {unmanaged > 0 && (
        <>
          <div className="side-title">Needs setup</div>
          {item({ kind: "unmanaged" }, <FolderSearch size={15} />, "Not managed yet")}
        </>
      )}
    </nav>
  );
}
