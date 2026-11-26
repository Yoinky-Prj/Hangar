import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { Check, CircleCheck, CircleX, GitBranch, Info, LoaderCircle, X } from "lucide-react";
import type { Category, GitStatus, JobProgress, Section } from "../api";
import { progressLabel, progressPct, uncommitted, type SubIssues } from "../util";

/* ---------- toasts ---------- */

type ToastKind = "ok" | "error" | "info";
interface ToastItem {
  id: number;
  kind: ToastKind;
  text: string;
}

const ToastCtx = createContext<(kind: ToastKind, text: string) => void>(() => {});

export function useToast() {
  return useContext(ToastCtx);
}

export function ToastProvider({ children }: { children: ReactNode }) {
  const [items, setItems] = useState<ToastItem[]>([]);
  const dismiss = useCallback((id: number) => setItems((v) => v.filter((t) => t.id !== id)), []);
  const push = useCallback(
    (kind: ToastKind, text: string) => {
      const id = Date.now() + Math.random();
      setItems((v) => [...v.slice(-4), { id, kind, text }]);
      window.setTimeout(() => dismiss(id), kind === "error" ? 10000 : 4500);
    },
    [dismiss],
  );
  return (
    <ToastCtx.Provider value={push}>
      {children}
      <div className="toasts" aria-live="polite">
        {items.map((t) => (
          <div key={t.id} className={`toast ${t.kind}`}>
            {t.kind === "ok" ? (
              <CircleCheck size={16} color="var(--ok)" />
            ) : t.kind === "error" ? (
              <CircleX size={16} color="var(--danger)" />
            ) : (
              <Info size={16} color="var(--accent)" />
            )}
            <div style={{ flex: 1 }}>{t.text}</div>
            <button onClick={() => dismiss(t.id)} aria-label="Dismiss">
              <X size={14} />
            </button>
          </div>
        ))}
      </div>
    </ToastCtx.Provider>
  );
}

/* ---------- modal ---------- */

export function Modal({
  title,
  onClose,
  children,
  footer,
  tabs,
  width,
  tall,
  locked,
  zIndex,
}: {
  title: string;
  onClose: () => void;
  children: ReactNode;
  footer?: ReactNode;
  tabs?: ReactNode;
  width?: number;
  /** Fixed height, for dialogs whose content changes (tabs, loading lists). */
  tall?: boolean;
  /**
   * A job is running: everything except the footer is non-interactive, and
   * the dialog can't be closed (no ×, Escape or click outside).
   */
  locked?: boolean;
  /** Stack above other dialogs (the default backdrop is 50). */
  zIndex?: number;
}) {
  useEffect(() => {
    const k = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !locked) onClose();
    };
    window.addEventListener("keydown", k);
    return () => window.removeEventListener("keydown", k);
  }, [onClose, locked]);
  return (
    <div
      className="backdrop"
      style={zIndex ? { zIndex } : undefined}
      onMouseDown={(e) => {
        if (e.target === e.currentTarget && !locked) onClose();
      }}
    >
      <div className={`modal ${tall ? "tall" : ""}`} style={width ? { width } : undefined} role="dialog" aria-label={title} aria-busy={locked}>
        <div className="modal-head">
          <h2>{title}</h2>
          <div className="spacer" />
          <button
            className="btn ghost icon"
            onClick={onClose}
            aria-label="Close"
            disabled={locked}
            title={locked ? "Wait for it to finish, or cancel it" : undefined}
          >
            <X size={16} />
          </button>
        </div>
        <div className="modal-lock" inert={locked || undefined}>
          {tabs}
        </div>
        <div className="modal-body">
          <div className="modal-lock" inert={locked || undefined}>
            {children}
          </div>
        </div>
        {footer && <div className="modal-foot">{footer}</div>}
      </div>
    </div>
  );
}

export function Tabs<T extends string>({
  tabs,
  value,
  onChange,
  disabled,
}: {
  tabs: { id: T; label: string; icon?: ReactNode }[];
  value: T;
  onChange: (t: T) => void;
  disabled?: boolean;
}) {
  return (
    <div className="tabs" role="tablist">
      {tabs.map((t) => (
        <button
          key={t.id}
          role="tab"
          aria-selected={value === t.id}
          className={value === t.id ? "on" : ""}
          disabled={disabled && value !== t.id}
          onClick={() => onChange(t.id)}
        >
          {t.icon}
          {t.label}
        </button>
      ))}
    </div>
  );
}

/* ---------- context menu ---------- */

export type MenuItem =
  | { label: string; icon?: ReactNode; onClick: () => void; disabled?: boolean; danger?: boolean }
  | "sep";

export function ContextMenu({ x, y, items, onClose }: { x: number; y: number; items: MenuItem[]; onClose: () => void }) {
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ x, y });
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const r = el.getBoundingClientRect();
    setPos({ x: Math.min(x, window.innerWidth - r.width - 8), y: Math.min(y, window.innerHeight - r.height - 8) });
  }, [x, y]);
  useEffect(() => {
    const close = () => onClose();
    const key = (e: KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("mousedown", close);
    window.addEventListener("blur", close);
    window.addEventListener("keydown", key);
    window.addEventListener("resize", close);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("blur", close);
      window.removeEventListener("keydown", key);
      window.removeEventListener("resize", close);
    };
  }, [onClose]);
  return (
    <div ref={ref} className="ctx" style={{ left: pos.x, top: pos.y }} onMouseDown={(e) => e.stopPropagation()} role="menu">
      {items.map((it, i) =>
        it === "sep" ? (
          <hr key={i} />
        ) : (
          <button
            key={i}
            role="menuitem"
            disabled={it.disabled}
            style={it.danger ? { color: "var(--danger)" } : undefined}
            onClick={() => {
              onClose();
              it.onClick();
            }}
          >
            {it.icon}
            {it.label}
          </button>
        ),
      )}
    </div>
  );
}

/* ---------- pickers ---------- */

export function CategoryPicker({
  categories,
  value,
  onChange,
  onManage,
}: {
  categories: Category[];
  value: string[];
  onChange: (ids: string[]) => void;
  onManage?: () => void;
}) {
  if (!categories.length) {
    return (
      <span className="hint">
        No categories yet.{" "}
        {onManage && (
          <a href="#" onClick={(e) => (e.preventDefault(), onManage())}>
            Create one
          </a>
        )}
      </span>
    );
  }
  return (
    <div className="row" style={{ flexWrap: "wrap", gap: 6 }}>
      {categories.map((c) => {
        const on = value.includes(c.id);
        return (
          <button
            type="button"
            key={c.id}
            className={`cat-chip ${on ? "on" : ""}`}
            onClick={() => onChange(on ? value.filter((v) => v !== c.id) : [...value, c.id])}
          >
            <span className="dot" style={{ background: c.color }} />
            {c.name}
            {on && <Check size={12} />}
          </button>
        );
      })}
    </div>
  );
}

export function SectionPicker({
  value,
  onChange,
}: {
  value: Exclude<Section, "archive">;
  onChange: (s: Exclude<Section, "archive">) => void;
}) {
  return (
    <div className="segmented">
      <button type="button" className={value === "projects" ? "on" : ""} onClick={() => onChange("projects")}>
        Projects
      </button>
      <button type="button" className={value === "reference" ? "on" : ""} onClick={() => onChange("reference")}>
        Reference
      </button>
    </div>
  );
}

/* ---------- status ---------- */

/** `undefined` = still loading, `null` = couldn't be read. */
export function StatusBadges({
  isGit,
  status,
  subs,
}: {
  isGit: boolean;
  status: GitStatus | null | undefined;
  subs?: SubIssues | null;
}) {
  if (!isGit) return <span className="faint">Not a git repo</span>;
  if (status === undefined) return <LoaderCircle size={14} className="spin faint" />;
  if (status === null) return <span className="pill danger">Can't read repo</span>;
  const n = uncommitted(status);
  // Which branch, shown only when it's not the remote's default.
  const branch = status.detached ? (
    <span key="br" className="pill" title="Not on a branch (detached HEAD)">
      detached
    </span>
  ) : status.branch && status.defaultBranch && status.branch !== status.defaultBranch ? (
    <span key="br" className="pill branch-pill" title={`On branch ${status.branch} (the default is ${status.defaultBranch})`}>
      <GitBranch size={11} />
      <span className="branch-pill-name">{status.branch}</span>
    </span>
  ) : null;
  const out: ReactNode[] = [];
  if (status.unborn) out.push(<span key="u" className="pill">No commits yet</span>);
  if (status.conflicted) out.push(<span key="c" className="pill danger">{status.conflicted} conflicts</span>);
  if (n > 0)
    out.push(
      <span key="n" className="pill warn" title={`${status.staged} staged · ${status.modified} modified · ${status.untracked} untracked`}>
        ● {n} changed
      </span>,
    );
  if (status.unpushed > 0) {
    const plus = status.unpushedCapped ? "+" : "";
    out.push(
      status.remotes.length ? (
        <span key="p" className="pill accent" title="Commits that aren't on any remote branch">
          ↑ {status.unpushed}
          {plus} unpushed
        </span>
      ) : (
        <span key="p" className="pill violet" title="This repo has no remote, so these commits only exist here">
          {status.unpushed}
          {plus} local only
        </span>
      ),
    );
  }
  if (status.behind) out.push(<span key="b" className="pill" title="Commits on the upstream you don't have">↓ {status.behind}</span>);
  if (subs) {
    const parts = [
      subs.changed && `${subs.changed} changed`,
      subs.unpushed && `${subs.unpushed} unpushed`,
      subs.moved && `${subs.moved} moved`,
      subs.missing && `${subs.missing} not checked out`,
    ].filter(Boolean);
    if (parts.length)
      out.push(
        <span key="s" className={`pill ${subs.changed || subs.unpushed ? "warn" : ""}`} title="Submodules that need attention">
          subs: {parts.join(" · ")}
        </span>,
      );
  }
  if (!out.length) out.push(<span key="ok" className="pill ok">Clean</span>);
  return (
    <>
      {branch}
      {out}
    </>
  );
}

export function JobBar({ progress }: { progress: JobProgress | null }) {
  const pct = progressPct(progress);
  return (
    <div style={{ flex: 1, minWidth: 0 }}>
      <div className={`progress ${pct === null ? "indeterminate" : ""}`}>
        <div style={pct === null ? undefined : { width: `${pct}%` }} />
      </div>
      <div className="hint" style={{ marginTop: 4, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
        {progress ? progressLabel(progress) : "Starting…"}
      </div>
    </div>
  );
}
