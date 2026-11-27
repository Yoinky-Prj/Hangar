import type { GitStatus, JobProgress, ProjectEntry, RepoRef, SubmoduleInfo } from "./api";

/** "owner/repo", plus the host when it isn't GitHub. */
export function repoLabel(r: { host: string; owner: string; repo: string }): string {
  return `${r.owner}/${r.repo}${r.host !== "github.com" ? ` · ${r.host}` : ""}`;
}

/** What a project is called in the UI: its name, else owner/repo, else its folder. */
export function projectTitle(p: ProjectEntry): string {
  const name = p.meta?.name?.trim();
  if (name) return name;
  const r = p.meta?.source.remote;
  return r ? `${r.owner}/${r.repo}` : p.folder;
}

/** Second line under the title: owner/repo when the project has its own name. */
/**
 * The title in two parts, so a list can let the owner shrink first and keep
 * the repo name readable. `owner` is null for a custom name or a plain folder.
 */
export function projectTitleParts(p: ProjectEntry): { owner: string | null; name: string } {
  const name = p.meta?.name?.trim();
  if (name) return { owner: null, name };
  const r = p.meta?.source.remote;
  return r ? { owner: r.owner, name: r.repo } : { owner: null, name: p.folder };
}

export function projectSubtitle(p: ProjectEntry): string | null {
  const r = p.meta?.source.remote;
  if (p.meta?.name?.trim()) return r ? repoLabel(r) : null;
  return r && r.host !== "github.com" ? r.host : null;
}

export interface SubIssues {
  total: number;
  changed: number;
  unpushed: number;
  moved: number;
  missing: number;
}

/** What needs attention across a repo's submodules. */
export function subIssues(list: SubmoduleInfo[] | null | undefined): SubIssues | null {
  if (!list || list.length === 0) return null;
  const r: SubIssues = { total: list.length, changed: 0, unpushed: 0, moved: 0, missing: 0 };
  for (const s of list) {
    if (!s.initialized) {
      r.missing++;
      continue;
    }
    if (s.moved) r.moved++;
    if (s.status && uncommitted(s.status) > 0) r.changed++;
    if (s.status && s.status.unpushed > 0) r.unpushed++;
  }
  return r;
}

export function hasSubIssues(i: SubIssues | null): boolean {
  return !!i && i.changed + i.unpushed + i.moved + i.missing > 0;
}

export function baseName(p: string): string {
  const parts = p.split(/[\\/]+/).filter(Boolean);
  return parts[parts.length - 1] ?? p;
}

/** Windows paths: case-insensitive, either slash, trailing slash ignored. */
export function samePath(a: string, b: string): boolean {
  const n = (p: string) => p.replace(/[\\/]+$/, "").replace(/\//g, "\\").toLowerCase();
  return n(a) === n(b);
}

/** Long paths keep the drive and the last folders: `C:\…\Workspaces\Main`. */
export function shortPath(p: string, max = 48): string {
  if (p.length <= max) return p;
  const sep = p.includes("\\") ? "\\" : "/";
  const parts = p.split(/[\\/]+/);
  const head = parts[0];
  let tail = parts[parts.length - 1];
  for (let i = parts.length - 2; i > 0; i--) {
    const next = parts[i] + sep + tail;
    if (head.length + 2 + next.length > max) break;
    tail = next;
  }
  return `${head}${sep}…${sep}${tail}`;
}

export function repoWebUrl(r: RepoRef): string {
  return `https://${r.host}/${r.owner}/${r.repo}`;
}

export function relTime(iso: string | null | undefined): string {
  if (!iso) return "";
  const t = new Date(iso).getTime();
  if (Number.isNaN(t)) return "";
  const s = Math.round((Date.now() - t) / 1000);
  if (s < 60) return "just now";
  const m = Math.round(s / 60);
  if (m < 60) return `${m} min ago`;
  const h = Math.round(m / 60);
  if (h < 24) return `${h} h ago`;
  const d = Math.round(h / 24);
  if (d < 30) return `${d} d ago`;
  const mo = Math.round(d / 30);
  if (mo < 12) return `${mo} mo ago`;
  return `${Math.round(mo / 12)} y ago`;
}

/** "in 7 h", "in 3 d" for a time in the future. */
export function untilTime(iso: string | null | undefined): string {
  if (!iso) return "";
  const t = new Date(iso).getTime();
  if (Number.isNaN(t)) return "";
  const s = Math.round((t - Date.now()) / 1000);
  if (s <= 60) return "now";
  const m = Math.round(s / 60);
  if (m < 60) return `in ${m} min`;
  const h = Math.round(m / 60);
  if (h < 48) return `in ${h} h`;
  return `in ${Math.round(h / 24)} d`;
}

export function fmtDate(iso: string | null | undefined): string {
  if (!iso) return "";
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? "" : d.toLocaleString();
}

export function fmtShortDate(iso: string | null | undefined): string {
  if (!iso) return "";
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? "" : d.toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });
}

export function fmtBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(v < 10 ? 1 : 0)} ${units[i]}`;
}

export function uncommitted(s: GitStatus): number {
  return s.staged + s.modified + s.untracked + s.conflicted;
}

export function progressLabel(p: JobProgress): string {
  const frac = p.total > 0 ? ` ${p.current.toLocaleString()} / ${p.total.toLocaleString()}` : "";
  const sub = p.detail ? ` (${p.detail})` : "";
  switch (p.stage) {
    case "receiving":
      return `Downloading objects${frac} · ${fmtBytes(p.bytes)}${sub}`;
    case "resolving":
      return `Resolving deltas${frac}${sub}`;
    case "checkout":
      return `Checking out files${frac}`;
    case "submodule":
      return `Submodule ${p.detail ?? ""}`;
    case "connecting":
      return "Asking the remote for its default branch…";
    case "copying":
      return `Copying files${frac} · ${fmtBytes(p.bytes)}`;
    default:
      return p.stage;
  }
}

export function progressPct(p: JobProgress | null): number | null {
  if (!p || p.total <= 0) return null;
  return Math.min(100, Math.round((p.current / p.total) * 100));
}

export const CATEGORY_COLORS = [
  "#5b8def",
  "#3fb37f",
  "#e5a23b",
  "#e5604f",
  "#a374e8",
  "#3bb6c9",
  "#d9669b",
  "#8a94a6",
];

export function newJobId(): string {
  return crypto.randomUUID();
}
