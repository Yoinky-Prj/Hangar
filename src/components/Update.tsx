import { useEffect, useState, useSyncExternalStore } from "react";
import { createPortal } from "react-dom";
import { CircleArrowUp, ExternalLink, LoaderCircle, RefreshCw, RotateCw } from "lucide-react";
import { api, errText, onUpdateAvailable, onUpdateProgress, type UpdateInfo, type UpdateStatus } from "../api";
import { onJobsChange, runningJobs } from "../jobs";
import { fmtBytes, fmtDate } from "../util";
import { Modal } from "./common";

const RELEASES_URL = "https://github.com/Yoinky-Prj/Hangar/releases";

/* ---------- this window's view of the update state ---------- */

let status: UpdateStatus | null = null;
let started = false;
const subscribers = new Set<() => void>();

function publish(s: UpdateStatus) {
  status = s;
  for (const f of subscribers) f();
}

function setAvailable(u: UpdateInfo | null) {
  if (status) publish({ ...status, available: u });
}

function start() {
  if (started) return;
  started = true;
  // The backend checks at start and every few hours, and tells every window.
  onUpdateAvailable(setAvailable);
  api
    .updateStatus()
    .then((s) => publish(status?.available && !s.available ? { ...s, available: status.available } : s))
    .catch(() => {});
}

function subscribe(f: () => void) {
  subscribers.add(f);
  return () => subscribers.delete(f);
}

/** Version, whether this copy updates itself, and a waiting update; null while loading. */
export function useUpdates(): UpdateStatus | null {
  start();
  return useSyncExternalStore(subscribe, () => status);
}

/* ---------- title bar button ---------- */

/** Shows only when an installed Hangar has a new version waiting. */
export function UpdateButton() {
  const s = useUpdates();
  const [open, setOpen] = useState(false);
  if (!s?.installed || !s.available) return null;
  return (
    <>
      <button
        className="btn small primary update-btn"
        title={`Hangar ${s.available.version} is available (you have ${s.current})`}
        onClick={() => setOpen(true)}
      >
        <CircleArrowUp size={14} />
        Update
      </button>
      {open && <UpdateDialog current={s.current} info={s.available} onClose={() => setOpen(false)} />}
    </>
  );
}

/* ---------- the update itself ---------- */

export function UpdateDialog({ current, info, onClose }: { current: string; info: UpdateInfo; onClose: () => void }) {
  const [phase, setPhase] = useState<"ask" | "downloading" | "installing">("ask");
  const [progress, setProgress] = useState<{ done: number; total: number | null }>({ done: 0, total: null });
  const [error, setError] = useState<string | null>(null);
  const [jobs, setJobs] = useState(runningJobs());

  useEffect(() => onJobsChange(() => setJobs(runningJobs())), []);

  useEffect(() => {
    let off: (() => void) | undefined;
    let alive = true;
    onUpdateProgress((done, total) => {
      setProgress({ done, total });
      if (total && done >= total) setPhase("installing");
    }).then((f) => (alive ? (off = f) : f()));
    return () => {
      alive = false;
      off?.();
    };
  }, []);

  async function install() {
    setError(null);
    setPhase("downloading");
    try {
      // Doesn't come back on success: Hangar exits once the installer starts.
      await api.installUpdate();
      setPhase("installing");
    } catch (e) {
      setError(errText(e));
      setPhase("ask");
    }
  }

  const working = phase !== "ask";
  const pct = progress.total ? Math.min(100, Math.round((progress.done / progress.total) * 100)) : null;
  return createPortal(
    <Modal
      title={`Update to Hangar ${info.version}`}
      onClose={onClose}
      width={540}
      locked={working}
      zIndex={80}
      footer={
        <>
          {working ? (
            <div style={{ flex: 1, minWidth: 0 }}>
              <div className={`progress ${pct === null ? "indeterminate" : ""}`}>
                <div style={pct === null ? undefined : { width: `${pct}%` }} />
              </div>
              <div className="hint" style={{ marginTop: 4 }}>
                {phase === "installing"
                  ? "Starting the installer…"
                  : progress.total
                    ? `Downloading ${fmtBytes(progress.done)} of ${fmtBytes(progress.total)}`
                    : "Downloading…"}
              </div>
            </div>
          ) : (
            <div className="spacer" />
          )}
          <button className="btn" onClick={onClose} disabled={working}>
            Later
          </button>
          <button className="btn primary" onClick={install} disabled={working || jobs.length > 0}>
            {working ? <LoaderCircle size={14} className="spin" /> : <RotateCw size={14} />}
            Restart and update
          </button>
        </>
      }
    >
      <div className="kv" style={{ marginBottom: 12 }}>
        <span className="k">You have</span>
        <span className="v">{current}</span>
        <span className="k">New version</span>
        <span className="v">
          <b>{info.version}</b>
          {info.date && <span className="faint"> · released {fmtDate(info.date)}</span>}
        </span>
      </div>
      {info.notes && <div className="update-notes selectable">{info.notes}</div>}
      <div className="hint">
        Hangar downloads the installer, closes all its windows, updates itself and starts again. Your workspaces and settings stay as
        they are.
      </div>
      {jobs.length > 0 && (
        <div className="callout warn" style={{ marginTop: 12 }}>
          <div>Still running in this window: {jobs.map((j) => j.label).join(", ")}. Let it finish (or cancel it) first.</div>
        </div>
      )}
      {error && (
        <div className="callout danger" style={{ marginTop: 12 }}>
          <div>{error}</div>
        </div>
      )}
    </Modal>,
    document.body,
  );
}

/* ---------- Settings > About ---------- */

export function AboutTab() {
  const s = useUpdates();
  const [checking, setChecking] = useState(false);
  const [upToDate, setUpToDate] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [open, setOpen] = useState(false);

  async function check() {
    setChecking(true);
    setError(null);
    setUpToDate(false);
    try {
      const found = await api.checkForUpdate();
      setAvailable(found);
      setUpToDate(!found);
    } catch (e) {
      setError(errText(e));
    } finally {
      setChecking(false);
    }
  }

  if (!s) return <LoaderCircle size={16} className="spin" />;
  return (
    <>
      <div className="kv" style={{ marginBottom: 16 }}>
        <span className="k">Version</span>
        <span className="v">{s.current}</span>
        <span className="k">Copy</span>
        <span className="v">{s.installed ? "Installed (updates itself)" : "Standalone exe (doesn't update itself)"}</span>
      </div>
      {s.installed ? (
        <>
          {s.available ? (
            <div className="callout" style={{ alignItems: "center" }}>
              <CircleArrowUp size={15} color="var(--accent)" />
              <div style={{ flex: 1 }}>
                Hangar <b>{s.available.version}</b> is available.
              </div>
              <button className="btn small primary" onClick={() => setOpen(true)}>
                Update…
              </button>
            </div>
          ) : (
            <div className="row">
              <button className="btn" onClick={check} disabled={checking}>
                {checking ? <LoaderCircle size={14} className="spin" /> : <RefreshCw size={14} />}
                Check for updates
              </button>
              {upToDate && <span className="hint">You have the latest version.</span>}
            </div>
          )}
          <div className="hint" style={{ marginTop: 12 }}>
            Hangar looks for a new version on GitHub when it starts and every few hours, and shows an Update button in the title bar when
            there is one.
          </div>
        </>
      ) : (
        <div className="hint">
          New versions are published on GitHub. The installer keeps Hangar up to date by itself; a standalone exe has to be replaced by
          hand.
          <div style={{ marginTop: 8 }}>
            <button className="btn small" onClick={() => api.openUrl(RELEASES_URL)}>
              <ExternalLink size={13} />
              Open releases
            </button>
          </div>
        </div>
      )}
      {error && (
        <div className="callout danger" style={{ marginTop: 12 }}>
          <div>{error}</div>
        </div>
      )}
      {open && s.available && <UpdateDialog current={s.current} info={s.available} onClose={() => setOpen(false)} />}
    </>
  );
}
