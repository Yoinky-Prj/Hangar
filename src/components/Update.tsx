import { useEffect, useState, useSyncExternalStore, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { ArrowRight, CircleArrowUp, CircleCheck, ExternalLink, LoaderCircle, RefreshCw, RotateCw } from "lucide-react";
import { api, errText, onUpdateAvailable, onUpdateInstalling, onUpdateProgress, type UpdateInfo, type UpdateStatus } from "../api";
import { onJobsChange, runningJobs } from "../jobs";
import { fmtBytes, fmtShortDate } from "../util";
import { AppIcon } from "./Brand";
import { Modal } from "./common";

const RELEASES_URL = "https://github.com/Yoinky-Prj/Hangar/releases";

/** Lightweight offline Markdown renderer for GitHub release notes (headings, bullets, bold, inline code). */
function renderInline(text: string): ReactNode[] {
  const parts = text.split(/(`[^`]+`|\*\*[^*]+\*\*)/g);
  return parts.map((p, i) => {
    if (p.startsWith("`") && p.endsWith("`") && p.length > 2) return <code key={i}>{p.slice(1, -1)}</code>;
    if (p.startsWith("**") && p.endsWith("**") && p.length > 4) return <strong key={i}>{p.slice(2, -2)}</strong>;
    return p;
  });
}

function MarkdownNotes({ text, className }: { text: string; className: string }) {
  const lines = text.replace(/\r\n/g, "\n").split("\n");
  const blocks: ReactNode[] = [];
  let bullets: string[] = [];

  const flushBullets = () => {
    if (!bullets.length) return;
    blocks.push(
      <ul key={`ul-${blocks.length}`}>
        {bullets.map((b, i) => (
          <li key={i}>{renderInline(b)}</li>
        ))}
      </ul>,
    );
    bullets = [];
  };

  for (const raw of lines) {
    const line = raw.trim();
    if (!line) {
      flushBullets();
      continue;
    }
    const h = line.match(/^#{1,4}\s+(.*)$/);
    if (h) {
      flushBullets();
      blocks.push(<h4 key={`h-${blocks.length}`}>{renderInline(h[1])}</h4>);
      continue;
    }
    const li = line.match(/^[-*]\s+(.*)$/);
    if (li) {
      bullets.push(li[1]);
      continue;
    }
    flushBullets();
    blocks.push(<p key={`p-${blocks.length}`}>{renderInline(line)}</p>);
  }
  flushBullets();

  return <div className={`${className} md-notes selectable`}>{blocks}</div>;
}

/* ---------- this window's view of the update state ---------- */

let status: UpdateStatus | null = null;
let started = false;
const subscribers = new Set<() => void>();

function publish(s: UpdateStatus) {
  status = s;
  for (const f of subscribers) f();
}

/** Reads the backend's state, which always has the result of its last check. */
function refresh() {
  api
    .updateStatus()
    .then(publish)
    .catch(() => {});
}

function setAvailable(u: UpdateInfo | null) {
  if (status) publish({ ...status, available: u });
  else refresh();
}

function start() {
  if (started) return;
  started = true;
  // The backend checks at start, when Hangar is started again and every hour,
  // and announces a new version. Listen first, then read, and re-read on each
  // announcement: a check finishing in between can't be missed that way.
  onUpdateAvailable(refresh).then(refresh, refresh);
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
        className="btn small update-btn"
        title={`Hangar ${s.available.version} is available (you have ${s.current})`}
        onClick={() => setOpen(true)}
      >
        <CircleArrowUp size={13} />
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
    let offInstalling: (() => void) | undefined;
    onUpdateProgress((done, total) => {
      setProgress({ done, total });
      if (total && done >= total) setPhase("installing");
    }).then((f) => (alive ? (off = f) : f()));
    onUpdateInstalling(() => setPhase("installing")).then((f) => (alive ? (offInstalling = f) : f()));
    return () => {
      alive = false;
      off?.();
      offInstalling?.();
    };
  }, []);

  async function install() {
    setError(null);
    setPhase("downloading");
    try {
      // Doesn't come back on success: Hangar restarts as the new version.
      await api.installUpdate();
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
                  ? `Installing Hangar ${info.version}; it reopens in a moment…`
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
      <div className="update-compare">
        <span className="pill mono">{current}</span>
        <ArrowRight size={14} style={{ color: "var(--text-muted)", flex: "none" }} />
        <span className="pill ok mono">{info.version}</span>
        {info.date && <span className="faint" style={{ marginLeft: "auto", fontSize: 11.5 }}>Released {fmtShortDate(info.date)}</span>}
      </div>
      {info.notes && <MarkdownNotes text={info.notes} className="update-notes" />}
      <div className="hint">
        Hangar downloads the update, installs it, and restarts where you left off. Your workspaces and settings stay as they are.
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
  /** Whose notes the changelog shows while an update is waiting. */
  const [shown, setShown] = useState<"new" | "installed">("new");

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
  const update = s.installed ? s.available : null;
  const newOne = update && shown === "new" ? update : null;
  const notes = newOne ? newOne.notes : s.currentNotes;
  const date = newOne ? newOne.date : s.currentDate;
  return (
    <>
      <div className="settings-pane-head">
        <h3>About</h3>
        <div className="hint">Version information, release notes, and automatic updates.</div>
      </div>

      <div className="about-hero">
        <AppIcon size={40} />
        <div style={{ flex: 1, minWidth: 0 }}>
          <div className="row" style={{ gap: 8 }}>
            <span className="about-title">Hangar</span>
            <span className="pill mono">{s.current}</span>
          </div>
          <div className="hint" style={{ marginTop: 2 }}>
            {s.installed ? "Installed · updates automatically" : "Portable exe · doesn't update"}
          </div>
        </div>
        {s.installed && !update && (
          <div className="row" style={{ gap: 8 }}>
            {upToDate && (
              <span className="pill ok">
                <CircleCheck size={11} />
                Up to date
              </span>
            )}
            <button className="btn small" onClick={check} disabled={checking}>
              {checking ? <LoaderCircle size={13} className="spin" /> : <RefreshCw size={13} />}
              Check for updates
            </button>
          </div>
        )}
      </div>

      <div className="card">
        <div className="row" style={{ marginBottom: 8 }}>
          {update ? (
            <div className="segmented">
              <button type="button" className={newOne ? "on" : ""} onClick={() => setShown("new")}>
                {update.version} · new
              </button>
              <button type="button" className={newOne ? "" : "on"} onClick={() => setShown("installed")}>
                {s.current} · installed
              </button>
            </div>
          ) : (
            <h4 style={{ margin: 0 }}>What's new in {s.current}</h4>
          )}
          <div className="spacer" />
          {update && (
            <button className="btn small primary" onClick={() => setOpen(true)}>
              <CircleArrowUp size={13} />
              Update to {update.version}
            </button>
          )}
        </div>
        {date && (
          <div className="hint" style={{ marginBottom: 8 }}>
            Released {fmtShortDate(date)}
          </div>
        )}
        {notes ? (
          <MarkdownNotes text={notes} className="release-notes" />
        ) : (
          <div className="hint">
            {newOne ? "No release notes." : "This build wasn't made from a GitHub release, so it has no release notes."}
          </div>
        )}
      </div>

      {s.installed ? (
        <div className="hint" style={{ marginTop: 12 }}>
          Hangar looks for a new version on GitHub when it starts and every hour, and shows an Update button in the title bar when
          there is one.
        </div>
      ) : (
        <div className="hint" style={{ marginTop: 12 }}>
          This portable copy can't update. For a newer version, download it from the releases page, or use the installer: an installed
          Hangar keeps itself up to date.
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
      {open && update && <UpdateDialog current={s.current} info={update} onClose={() => setOpen(false)} />}
    </>
  );
}
