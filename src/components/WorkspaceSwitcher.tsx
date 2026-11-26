import { useEffect, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent, type MouseEvent } from "react";
import { createPortal } from "react-dom";
import { open } from "@tauri-apps/plugin-dialog";
import { Check, ChevronDown, FolderPlus, LoaderCircle, Search, Warehouse, X } from "lucide-react";
import { api, errText, type Settings, type WorkspaceSummary, type WorkspaceView } from "../api";
import { runningJobs } from "../jobs";
import { samePath, shortPath } from "../util";
import { useToast } from "./common";

/**
 * The workspace name in the top bar: a dropdown listing every workspace
 * (searchable, with project counts) plus "Add workspace…".
 */
export function WorkspaceSwitcher({
  ws,
  setSettings,
  onOpen,
  onSetup,
}: {
  ws: WorkspaceView;
  setSettings: (s: Settings) => void;
  /** Open an existing workspace. */
  onOpen: (path: string) => void;
  /** A folder that isn't a workspace yet: ask before setting it up. */
  onSetup: (path: string, entryCount: number) => void;
}) {
  const toast = useToast();
  const [isOpen, setOpen] = useState(false);
  const [list, setList] = useState<WorkspaceSummary[] | null>(null);
  const [q, setQ] = useState("");
  const [cursor, setCursor] = useState(0);
  const [pos, setPos] = useState<{ left: number; top: number } | null>(null);
  const btnRef = useRef<HTMLButtonElement>(null);
  const popRef = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  useLayoutEffect(() => {
    if (!isOpen || !btnRef.current) return;
    const r = btnRef.current.getBoundingClientRect();
    setPos({ left: Math.max(8, Math.min(r.left - 4, window.innerWidth - 428)), top: r.bottom + 6 });
  }, [isOpen]);

  useEffect(() => {
    if (!isOpen) return;
    setQ("");
    setCursor(0);
    let alive = true;
    api
      .listWorkspaces()
      .then((l) => {
        if (!alive) return;
        setList(l);
        // Start the keyboard cursor on the workspace that's open.
        const i = l.findIndex((w) => samePath(w.path, ws.root));
        setCursor(Math.max(0, i));
      })
      .catch((e) => toast("error", errText(e)));
    return () => {
      alive = false;
    };
  }, [isOpen, ws.root, toast]);

  useEffect(() => {
    if (!isOpen) return;
    const down = (e: globalThis.MouseEvent) => {
      const t = e.target as Node;
      if (!popRef.current?.contains(t) && !btnRef.current?.contains(t)) setOpen(false);
    };
    const key = (e: globalThis.KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        setOpen(false);
      }
    };
    const close = () => setOpen(false);
    document.addEventListener("mousedown", down);
    document.addEventListener("keydown", key, true);
    window.addEventListener("resize", close);
    window.addEventListener("blur", close);
    return () => {
      document.removeEventListener("mousedown", down);
      document.removeEventListener("keydown", key, true);
      window.removeEventListener("resize", close);
      window.removeEventListener("blur", close);
    };
  }, [isOpen]);

  const shown = useMemo(() => {
    const words = q.trim().toLowerCase().split(/\s+/).filter(Boolean);
    return (list ?? []).filter((w) => {
      const hay = `${w.name} ${w.path}`.toLowerCase();
      return words.every((x) => hay.includes(x));
    });
  }, [list, q]);

  useEffect(() => {
    listRef.current?.querySelector(".ws-item.cursor")?.scrollIntoView({ block: "nearest" });
  }, [cursor, shown]);

  /** Switching mid-clone would cut the job off, so refuse while anything runs. */
  function blockedByJob(): boolean {
    const jobs = runningJobs();
    if (!jobs.length) return false;
    const more = jobs.length > 1 ? ` (and ${jobs.length - 1} more)` : "";
    toast("error", `Wait for "${jobs[0].label}"${more} to finish before switching workspaces.`);
    return true;
  }

  function pick(w: WorkspaceSummary) {
    if (samePath(w.path, ws.root)) {
      setOpen(false);
      return;
    }
    if (!w.exists) {
      toast("error", `"${w.name}" isn't there any more (${shortPath(w.path, 60)}). Remove it from the list with ×, or reconnect its drive.`);
      return;
    }
    if (blockedByJob()) return;
    setOpen(false);
    if (w.isWorkspace) onOpen(w.path);
    else onSetup(w.path, w.entryCount);
  }

  async function add() {
    if (blockedByJob()) return;
    setOpen(false);
    const dir = await open({ directory: true, multiple: false, title: "Choose a workspace folder, or a folder to turn into one" });
    if (typeof dir !== "string" || samePath(dir, ws.root)) return;
    try {
      const insp = await api.inspectFolder(dir);
      if (insp.isWorkspace) onOpen(dir);
      else onSetup(dir, insp.entryCount);
    } catch (e) {
      toast("error", errText(e));
    }
  }

  async function remove(w: WorkspaceSummary, e: MouseEvent) {
    e.stopPropagation();
    try {
      setSettings(await api.forgetWorkspace(w.path));
      setList((l) => l && l.filter((x) => x.path !== w.path));
    } catch (err) {
      toast("error", errText(err));
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setCursor((c) => Math.min(shown.length - 1, c + 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setCursor((c) => Math.max(0, c - 1));
    } else if (e.key === "Enter" && shown[cursor]) {
      e.preventDefault();
      pick(shown[cursor]);
    }
  }

  const pop =
    isOpen && pos
      ? createPortal(
          <div className="ws-pop" ref={popRef} role="dialog" aria-label="Workspaces" style={{ left: pos.left, top: pos.top }}>
            <div className="ws-pop-search">
              <Search size={14} />
              <input
                className="input"
                autoFocus
                placeholder="Search workspaces"
                value={q}
                onChange={(e) => {
                  setQ(e.target.value);
                  setCursor(0);
                }}
                onKeyDown={onKey}
              />
            </div>
            <div className="ws-pop-list" ref={listRef}>
              {list === null ? (
                <div className="ws-pop-empty">
                  <LoaderCircle size={14} className="spin" /> Loading…
                </div>
              ) : shown.length === 0 ? (
                <div className="ws-pop-empty">{list.length ? "No workspaces match." : "No workspaces yet."}</div>
              ) : (
                shown.map((w, i) => {
                  const current = samePath(w.path, ws.root);
                  return (
                    <div
                      key={w.path}
                      role="option"
                      aria-selected={current}
                      className={`ws-item ${i === cursor ? "cursor" : ""} ${current ? "current" : ""} ${!w.exists ? "missing" : ""}`}
                      onMouseEnter={() => setCursor(i)}
                      onClick={() => pick(w)}
                      title={w.path}
                    >
                      <div className="ws-item-icon">
                        <Warehouse size={15} />
                      </div>
                      <div className="ws-item-text">
                        <div className="ws-item-name">
                          <span className="ws-item-label">{w.name}</span>
                          {!w.exists && <span className="pill danger">not found</span>}
                          {w.exists && !w.isWorkspace && <span className="pill">not set up</span>}
                        </div>
                        <div className="ws-item-path">{shortPath(w.path, 46)}</div>
                      </div>
                      {w.isWorkspace && (
                        <span className="ws-item-count" title={`${w.projects} project folder${w.projects === 1 ? "" : "s"}, archive included`}>
                          {w.projects}
                        </span>
                      )}
                      {current ? (
                        <Check size={15} className="ws-item-check" aria-label="Open now" />
                      ) : (
                        <button
                          className="ws-item-remove"
                          title="Remove from the list (the folder itself isn't touched)"
                          aria-label="Remove from the list"
                          onClick={(e) => remove(w, e)}
                        >
                          <X size={13} />
                        </button>
                      )}
                    </div>
                  );
                })
              )}
            </div>
            <div className="ws-pop-foot">
              <button className="ws-pop-add" onClick={add}>
                <FolderPlus size={15} />
                Add workspace…
              </button>
            </div>
          </div>,
          document.body,
        )
      : null;

  return (
    <>
      <button
        ref={btnRef}
        className={`ws-switch ${isOpen ? "on" : ""}`}
        onClick={() => setOpen((o) => !o)}
        title={`${ws.root}\nClick to switch or add workspaces`}
        aria-haspopup="dialog"
        aria-expanded={isOpen}
      >
        <span className="ws-switch-name">{ws.config.name}</span>
        <ChevronDown size={13} />
      </button>
      {pop}
    </>
  );
}
