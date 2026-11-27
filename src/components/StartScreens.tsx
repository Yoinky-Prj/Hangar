import { useEffect, useState, type ReactNode } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { Archive, ArrowLeft, BookOpen, Briefcase, ChevronRight, CircleX, FolderOpen, LoaderCircle, Warehouse, X } from "lucide-react";
import { api, errText, type Environment, type FolderInspection, type Settings, type WorkspaceSummary, type WorkspaceView } from "../api";
import { AppIcon } from "./Brand";
import { useToast } from "./common";
import { Frame } from "./TitleBar";
import { shortPath } from "../util";
import { SetupPreview } from "./WorkspaceSetup";

export function Splash() {
  return (
    <Frame brand={false}>
      <div className="center-screen">
        <div className="splash-box">
          <AppIcon size={54} className="splash-icon" />
          <div className="splash-meta">
            <LoaderCircle size={15} className="spin" />
            <span>Opening Hangar…</span>
          </div>
        </div>
      </div>
    </Frame>
  );
}

export function StartError({ message }: { message: string }) {
  return (
    <Frame>
      <div className="center-screen">
        <div className="panel-card" style={{ width: 480, maxWidth: "100%" }}>
          <div className="row" style={{ gap: 10, marginBottom: 12 }}>
            <CircleX size={20} color="var(--danger)" />
            <h2 style={{ margin: 0 }}>Hangar couldn't start</h2>
          </div>
          <div className="callout danger selectable">
            <div>{message}</div>
          </div>
        </div>
      </div>
    </Frame>
  );
}

/** Shown only when a required tool is missing. Hangar can't be used until it's fixed. */
export function EnvBlocker({ env, onRetry }: { env: Environment; onRetry: () => void }) {
  const missing = env.tools.filter((t) => t.required && !t.found);
  return (
    <Frame>
      <div className="center-screen">
        <div className="panel-card" style={{ width: 520, maxWidth: "100%" }}>
          <h2>Hangar can't start yet</h2>
          <p className="muted">Install what's missing below, then check again.</p>
          {missing.map((t) => (
            <div key={t.name} className="tool-row blocker-tool">
              <CircleX size={18} color="var(--danger)" />
              <div>
                <b>{t.name}</b>
                <div className="hint">{t.note}</div>
              </div>
            </div>
          ))}
          <button className="btn primary lg block" onClick={onRetry} style={{ marginTop: 18 }}>
            Check again
          </button>
        </div>
      </div>
    </Frame>
  );
}

function Bay({ icon, tone, name, text }: { icon: ReactNode; tone: string; name: string; text: string }) {
  return (
    <div className="bay">
      <div className={`bay-icon ${tone}`}>{icon}</div>
      <div>
        <div className="bay-name">{name}</div>
        <div className="bay-text">{text}</div>
      </div>
    </div>
  );
}

export function WorkspacePicker({
  settings,
  setSettings,
  onOpened,
}: {
  settings: Settings;
  setSettings: (s: Settings) => void;
  onOpened: (ws: WorkspaceView) => void;
}) {
  const toast = useToast();
  const [pending, setPending] = useState<{ path: string; insp: FolderInspection } | null>(null);
  const [busy, setBusy] = useState(false);
  const [list, setList] = useState<WorkspaceSummary[] | null>(null);

  useEffect(() => {
    let alive = true;
    api
      .listWorkspaces()
      .then((l) => alive && setList(l))
      .catch(() => alive && setList([]));
    return () => {
      alive = false;
    };
  }, [settings.workspaces]);

  async function consider(path: string) {
    try {
      const insp = await api.inspectFolder(path);
      if (!insp.exists) {
        toast("error", `${path} doesn't exist any more.`);
        return;
      }
      if (insp.isWorkspace) {
        setBusy(true);
        onOpened(await api.openWorkspace(path));
      } else {
        setPending({ path, insp });
      }
    } catch (e) {
      toast("error", errText(e));
    } finally {
      setBusy(false);
    }
  }

  async function choose() {
    const dir = await open({ directory: true, multiple: false, title: "Choose a workspace folder" });
    if (typeof dir === "string") await consider(dir);
  }

  async function init() {
    if (!pending) return;
    setBusy(true);
    try {
      onOpened(await api.initWorkspace(pending.path));
    } catch (e) {
      toast("error", errText(e));
      setBusy(false);
    }
  }

  return (
    <Frame brand={false} className="landing">
      <div className="landing-body">
        <section className="landing-hero">
          <div className="hero-brand">
            <AppIcon size={64} className="hero-icon" />
            <div>
              <h1>Hangar</h1>
              <div className="hero-sub">Local code workspace dashboard</div>
            </div>
          </div>
          <p className="tagline">All your projects in one folder: your own work, the code you learn from, and everything you've parked.</p>
          <div className="bays">
            <Bay icon={<Briefcase size={17} />} tone="blue" name="Projects" text="Your repos and your organizations' repos" />
            <Bay icon={<BookOpen size={17} />} tone="teal" name="Reference" text="Other people's code you keep around to read" />
            <Bay icon={<Archive size={17} />} tone="amber" name="Archive" text="Finished or paused work, out of the way but one click back" />
          </div>
          <div className="hero-foot">
            <span className="hero-foot-dot" />
            Git and GitHub built in. Nothing else to install.
          </div>
        </section>

        <section className="landing-panel">
          {pending ? (
            <div className="panel-card" key="setup">
              <button className="link-back" onClick={() => setPending(null)} disabled={busy}>
                <ArrowLeft size={14} />
                Back
              </button>
              <h2>Set up a new workspace</h2>
              <SetupPreview path={pending.path} entryCount={pending.insp.entryCount} />
              <button className="btn primary lg block" onClick={init} disabled={busy} style={{ marginTop: 20 }}>
                {busy && <LoaderCircle size={15} className="spin" />}
                Create workspace
              </button>
            </div>
          ) : (
            <div className="panel-card" key="open">
              <h2>Open a workspace</h2>
              <p className="muted">Pick a folder that's already a Hangar workspace, or any folder to turn into one.</p>
              <button className="btn primary lg block" onClick={choose} disabled={busy}>
                {busy ? <LoaderCircle size={15} className="spin" /> : <FolderOpen size={16} />}
                Choose folder…
              </button>

              <div className="panel-label">Workspaces</div>
              {list === null ? null : list.length === 0 ? (
                <div className="recent-empty">Workspaces you add show up here.</div>
              ) : (
                <div className="recent-list">
                  {list.map((st) => {
                    const p = st.path;
                    const missing = !st.exists;
                    return (
                      <div
                        key={p}
                        role="button"
                        tabIndex={0}
                        className={`recent-row ${missing ? "missing" : ""}`}
                        title={p}
                        onClick={() => !busy && consider(p)}
                        onKeyDown={(e) => {
                          if ((e.key === "Enter" || e.key === " ") && !busy) {
                            e.preventDefault();
                            consider(p);
                          }
                        }}
                      >
                        <div className="recent-icon">
                          <Warehouse size={16} />
                        </div>
                        <div className="recent-text">
                          <div className="recent-name">
                            <span className="recent-label">{st.name}</span>
                            {missing && <span className="pill danger">not found</span>}
                            {st.exists && !st.isWorkspace && <span className="pill">not set up</span>}
                          </div>
                          <div className="recent-path">{shortPath(p, 56)}</div>
                        </div>
                        {st.isWorkspace && (
                          <span className="ws-item-count" title={`${st.projects} project folder${st.projects === 1 ? "" : "s"}, archive included`}>
                            {st.projects}
                          </span>
                        )}
                        <button
                          className="recent-remove"
                          title="Remove from list"
                          aria-label="Remove from list"
                          onClick={async (e) => {
                            e.stopPropagation();
                            setSettings(await api.forgetWorkspace(p));
                          }}
                        >
                          <X size={14} />
                        </button>
                        <ChevronRight size={16} className="recent-chev" />
                      </div>
                    );
                  })}
                </div>
              )}
            </div>
          )}
        </section>
      </div>
    </Frame>
  );
}
