import { useEffect, useRef, useState } from "react";
import { CircleCheck, CircleX, Copy, ExternalLink, Info, KeyRound, LoaderCircle, LogOut, Pencil, Plus, Tag, Trash, Warehouse, Wrench } from "lucide-react";
import {
  api,
  errText,
  type Account,
  type DeviceCode,
  type Environment,
  type ProjectEntry,
  type Settings,
  type TokenInfo,
  type WorkspaceConfig,
  type WorkspaceView,
} from "../api";
import { CATEGORY_COLORS, fmtDate, untilTime } from "../util";
import { Modal, Tabs, useToast } from "./common";
import { AboutTab } from "./Update";

export type SettingsTab = "github" | "categories" | "environment" | "workspace" | "about";

/* ---------- GitHub ---------- */

function TokenLine({ token }: { token: TokenInfo }) {
  if (token.kind === "pat")
    return <>Personal access token · {token.patExpires ? `expires ${token.patExpires}` : "no expiry date"}</>;
  if (!token.expiresAt) return <>Signed in through GitHub · token doesn't expire</>;
  if (token.renews)
    return (
      <>
        Short-lived token, renewed automatically (current one ends {untilTime(token.expiresAt)})
        {token.renewUntil && <> · sign in again if Hangar isn't used before {fmtDate(token.renewUntil)}</>}
      </>
    );
  return <>Short-lived token, ends {untilTime(token.expiresAt)}; sign in again after that</>;
}

function GitHubTab({
  settings,
  setSettings,
  account,
  accountError,
  setAccount,
}: {
  settings: Settings;
  setSettings: (s: Settings) => void;
  account: Account | null | undefined;
  accountError: string | null;
  setAccount: (a: Account | null) => void;
}) {
  const toast = useToast();
  const [clientId, setClientId] = useState(settings.githubClientId ?? "");
  const [device, setDevice] = useState<DeviceCode | null>(null);
  const [token, setToken] = useState("");
  const [busy, setBusy] = useState(false);
  const timer = useRef<number | null>(null);

  useEffect(
    () => () => {
      if (timer.current) window.clearTimeout(timer.current);
    },
    [],
  );

  function stopPolling() {
    if (timer.current) window.clearTimeout(timer.current);
    timer.current = null;
    setDevice(null);
  }

  function poll(d: DeviceCode, interval: number) {
    timer.current = window.setTimeout(async () => {
      try {
        const r = await api.githubPollDevice(d.deviceCode);
        if (r.status === "pending") poll(d, interval);
        else if (r.status === "slowDown") poll(d, r.interval);
        else if (r.status === "done") {
          stopPolling();
          setAccount(r.account);
          toast("ok", `Signed in to GitHub as ${r.account.login}.`);
        } else {
          stopPolling();
          toast("error", r.status === "expired" ? "The code expired. Start again." : "Sign-in was cancelled on GitHub.");
        }
      } catch (e) {
        stopPolling();
        toast("error", errText(e));
      }
    }, interval * 1000);
  }

  async function startDevice() {
    setBusy(true);
    try {
      if ((settings.githubClientId ?? "") !== clientId.trim()) setSettings(await api.setGithubClientId(clientId));
      const d = await api.githubStartDevice();
      setDevice(d);
      poll(d, d.interval);
    } catch (e) {
      toast("error", errText(e));
    } finally {
      setBusy(false);
    }
  }

  async function saveToken() {
    setBusy(true);
    try {
      const a = await api.githubSignInToken(token);
      setToken("");
      setAccount(a);
      toast("ok", `Signed in to GitHub as ${a.login}.`);
    } catch (e) {
      toast("error", errText(e));
    } finally {
      setBusy(false);
    }
  }

  async function signOut() {
    try {
      await api.githubSignOut();
      setAccount(null);
      toast("ok", "Signed out of GitHub. The token was removed from this computer.");
    } catch (e) {
      toast("error", errText(e));
    }
  }

  if (account === undefined)
    return (
      <div className="hint row">
        <LoaderCircle size={14} className="spin" /> Checking…
      </div>
    );

  if (account)
    return (
      <>
        <div className="card row" style={{ gap: 14 }}>
          {account.avatarUrl && <img src={account.avatarUrl} alt="" width={48} height={48} style={{ borderRadius: "50%" }} />}
          <div style={{ flex: 1 }}>
            <div style={{ fontSize: 15, fontWeight: 700 }}>{account.login}</div>
            {account.name && <div className="muted">{account.name}</div>}
            <div className="hint" style={{ marginTop: 4 }}>
              <TokenLine token={account.token} />
            </div>
            {account.scopes && <div className="hint">Token scopes: {account.scopes || "none"}</div>}
          </div>
          <button className="btn" onClick={signOut}>
            <LogOut size={14} />
            Sign out
          </button>
        </div>
        <div className="field">
          <label>Organizations</label>
          {account.orgs.length ? (
            <div className="row" style={{ flexWrap: "wrap", gap: 6 }}>
              {account.orgs.map((o) => (
                <span key={o} className="pill">
                  {o}
                </span>
              ))}
            </div>
          ) : (
            <span className="hint">None visible to this sign-in.</span>
          )}
          <span className="hint">
            Repos you own and repos from these organizations go to <b>Projects</b> by default; everything else to <b>Reference</b>.
            If an organization's repos are missing, an owner of that organization may need to approve your OAuth App (or authorize
            your token for SSO).
          </span>
        </div>
      </>
    );

  return (
    <>
      {accountError && (
        <div className="callout danger" style={{ marginBottom: 12 }}>
          <CircleX size={15} />
          <div>{accountError}</div>
        </div>
      )}
      <div className="card">
        <h4>Sign in with a device code (recommended)</h4>
        <div className="hint" style={{ marginBottom: 10 }}>
          You approve Hangar on github.com; no password or token is typed here. This needs a free GitHub OAuth App, made once:
          github.com → Settings → Developer settings → OAuth Apps → New OAuth App. Any homepage and callback URL works (for example{" "}
          <span className="mono">http://localhost</span>). Tick <b>Enable Device Flow</b>, then copy its Client ID here.
        </div>
        {device ? (
          <>
            <div className="hint" style={{ marginBottom: 6 }}>
              Open the GitHub page and enter this code:
            </div>
            <div className="row" style={{ marginBottom: 10 }}>
              <span className="device-code">{device.userCode}</span>
              <button className="btn icon" title="Copy code" onClick={() => navigator.clipboard.writeText(device.userCode)}>
                <Copy size={14} />
              </button>
            </div>
            <div className="row">
              <button className="btn primary" onClick={() => api.openUrl(device.verificationUri)}>
                <ExternalLink size={14} />
                Open {device.verificationUri.replace("https://", "")}
              </button>
              <span className="hint row">
                <LoaderCircle size={13} className="spin" /> Waiting for approval…
              </span>
              <span className="spacer" />
              <button className="btn" onClick={stopPolling}>
                Cancel
              </button>
            </div>
          </>
        ) : (
          <>
            <div className="row">
              <input
                className="input mono"
                placeholder="OAuth App Client ID (e.g. Ov23li…)"
                value={clientId}
                onChange={(e) => setClientId(e.target.value)}
              />
              <button className="btn primary" onClick={startDevice} disabled={busy || !clientId.trim()}>
                Sign in
              </button>
            </div>
            <div className="hint" style={{ marginTop: 10 }}>
              Works with token expiration on or off in your OAuth App. Short-lived tokens are renewed automatically.
            </div>
          </>
        )}
      </div>
      <div className="card">
        <h4>Or use a personal access token</h4>
        <div className="hint" style={{ marginBottom: 10 }}>
          Create one at github.com → Settings → Developer settings → Personal access tokens. A classic token needs the{" "}
          <span className="mono">repo</span> and <span className="mono">read:org</span> scopes. It is stored in Windows Credential
          Manager, not in a file.
        </div>
        <div className="row">
          <input
            className="input mono"
            type="password"
            placeholder="ghp_… or github_pat_…"
            value={token}
            onChange={(e) => setToken(e.target.value)}
            autoComplete="off"
          />
          <button className="btn" onClick={saveToken} disabled={busy || !token.trim()}>
            <KeyRound size={14} />
            Save token
          </button>
        </div>
      </div>
    </>
  );
}

/* ---------- categories ---------- */

function CategoryRow({
  ws,
  id,
  name,
  color,
  usage,
  onConfig,
}: {
  ws: WorkspaceView;
  id: string;
  name: string;
  color: string;
  usage: number;
  onConfig: (c: WorkspaceConfig) => void;
}) {
  const toast = useToast();
  const [n, setN] = useState(name);
  const [c, setC] = useState(color);
  const [confirm, setConfirm] = useState(false);
  const dirty = n.trim() !== name || c !== color;

  async function save() {
    try {
      onConfig(await api.updateCategory(ws.root, id, n, c));
    } catch (e) {
      toast("error", errText(e));
    }
  }

  return (
    <div className="row" style={{ padding: "6px 0", borderBottom: "1px solid var(--border)" }}>
      <input type="color" value={c} onChange={(e) => setC(e.target.value)} title="Category color" />
      <input className="input" value={n} onChange={(e) => setN(e.target.value)} onKeyDown={(e) => e.key === "Enter" && dirty && save()} />
      <span className="hint" style={{ width: 80, textAlign: "right" }}>
        {usage} project{usage === 1 ? "" : "s"}
      </span>
      {dirty && (
        <button className="btn small primary" onClick={save}>
          <Pencil size={12} />
          Save
        </button>
      )}
      {confirm ? (
        <>
          <button
            className="btn small danger"
            onClick={async () => {
              try {
                onConfig(await api.deleteCategory(ws.root, id));
              } catch (e) {
                toast("error", errText(e));
              }
            }}
          >
            Delete
          </button>
          <button className="btn small" onClick={() => setConfirm(false)}>
            Keep
          </button>
        </>
      ) : (
        <button className="btn small ghost icon" title="Delete category" onClick={() => setConfirm(true)}>
          <Trash size={13} />
        </button>
      )}
    </div>
  );
}

function CategoriesTab({ ws, projects, onConfig }: { ws: WorkspaceView; projects: ProjectEntry[]; onConfig: (c: WorkspaceConfig) => void }) {
  const toast = useToast();
  const [name, setName] = useState("");
  const [color, setColor] = useState(CATEGORY_COLORS[ws.config.categories.length % CATEGORY_COLORS.length]);

  async function add() {
    try {
      const cfg = await api.addCategory(ws.root, name, color);
      onConfig(cfg);
      setName("");
      setColor(CATEGORY_COLORS[cfg.categories.length % CATEGORY_COLORS.length]);
    } catch (e) {
      toast("error", errText(e));
    }
  }

  return (
    <>
      <div className="hint" style={{ marginBottom: 12 }}>
        Categories are tags. A project can have several, and changing them never moves any files. Deleting a category removes it from
        its projects.
      </div>
      {ws.config.categories.map((c) => (
        <CategoryRow
          key={c.id}
          ws={ws}
          id={c.id}
          name={c.name}
          color={c.color}
          usage={projects.filter((p) => p.meta?.categories.includes(c.id)).length}
          onConfig={onConfig}
        />
      ))}
      <div className="row" style={{ paddingTop: 12 }}>
        <input type="color" value={color} onChange={(e) => setColor(e.target.value)} title="Category color" />
        <input
          className="input"
          placeholder="New category, e.g. Bots"
          value={name}
          onChange={(e) => setName(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && name.trim() && add()}
          autoFocus
        />
        <button className="btn primary" onClick={add} disabled={!name.trim()}>
          <Plus size={14} />
          Add
        </button>
      </div>
    </>
  );
}

/* ---------- environment ---------- */

function EnvironmentTab({ env }: { env: Environment }) {
  return (
    <>
      <div className="hint" style={{ marginBottom: 8 }}>
        Hangar has git and GitHub access built in, so it doesn't need Git or the GitHub CLI installed. The rest are optional helpers.
      </div>
      {env.tools.map((t) => (
        <div key={t.name} className="tool-row">
          {t.found ? <CircleCheck size={18} color="var(--ok)" /> : <CircleX size={18} color={t.required ? "var(--danger)" : "var(--faint)"} />}
          <div style={{ flex: 1 }}>
            <div className="row">
              <b>{t.name}</b>
              <span className={`pill ${t.required ? "accent" : ""}`}>{t.required ? "required" : "optional"}</span>
            </div>
            <div className="hint">{t.note}</div>
            {t.path && <div className="mono faint selectable">{t.path}</div>}
          </div>
        </div>
      ))}
    </>
  );
}

/* ---------- workspace ---------- */

function WorkspaceTab({ ws, projects, onRemove }: { ws: WorkspaceView; projects: ProjectEntry[]; onRemove: () => Promise<void> }) {
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  return (
    <>
      <div className="kv" style={{ marginBottom: 16 }}>
        <span className="k">Name</span>
        <span className="v">{ws.config.name}</span>
        <span className="k">Folder</span>
        <span className="v mono selectable">{ws.root}</span>
        <span className="k">Created</span>
        <span className="v">{fmtDate(ws.config.createdAt)}</span>
        <span className="k">Projects</span>
        <span className="v">
          {projects.length} folder{projects.length === 1 ? "" : "s"}
        </span>
      </div>
      <div className="hint" style={{ marginBottom: 12 }}>
        Each project's metadata lives in its own <span className="mono">.hangar</span> folder, which is kept out of git through{" "}
        <span className="mono">.git/info/exclude</span>. The workspace's <span className="mono">.hangar\index.json</span> keeps a backup, so
        a deleted and re-cloned repo gets its categories back.
      </div>
      <div className="ws-remove">
        <div className="ws-remove-text">
          <b>Remove from Hangar</b>
          <div className="hint">
            Hangar forgets this workspace and goes back to the start screen. Nothing on disk changes: the folder, its projects and its{" "}
            <span className="mono">.hangar</span> data stay, so adding it again brings back its categories and names.
          </div>
        </div>
        {confirming ? (
          <div className="ws-remove-actions">
            <button className="btn" onClick={() => setConfirming(false)} disabled={busy}>
              Cancel
            </button>
            <button
              className="btn danger"
              disabled={busy}
              onClick={async () => {
                setBusy(true);
                try {
                  await onRemove();
                } finally {
                  setBusy(false);
                  setConfirming(false);
                }
              }}
            >
              {busy ? <LoaderCircle size={14} className="spin" /> : <LogOut size={14} />}
              Remove "{ws.config.name}"
            </button>
          </div>
        ) : (
          <button className="btn" onClick={() => setConfirming(true)}>
            <LogOut size={14} />
            Remove…
          </button>
        )}
      </div>
    </>
  );
}

/* ---------- dialog ---------- */

export function SettingsDialog({
  initialTab,
  ws,
  projects,
  settings,
  setSettings,
  env,
  account,
  accountError,
  setAccount,
  onConfig,
  onRemove,
  onClose,
}: {
  initialTab: SettingsTab;
  ws: WorkspaceView;
  projects: ProjectEntry[];
  settings: Settings;
  setSettings: (s: Settings) => void;
  env: Environment;
  account: Account | null | undefined;
  accountError: string | null;
  setAccount: (a: Account | null) => void;
  onConfig: (c: WorkspaceConfig) => void;
  /** Forget this workspace and go back to the start screen. */
  onRemove: () => Promise<void>;
  onClose: () => void;
}) {
  const [tab, setTab] = useState<SettingsTab>(initialTab);
  return (
    <Modal
      title="Settings"
      onClose={onClose}
      width={720}
      tall
      tabs={
        <Tabs<SettingsTab>
          value={tab}
          onChange={setTab}
          tabs={[
            { id: "github", label: "GitHub", icon: <KeyRound size={14} /> },
            { id: "categories", label: "Categories", icon: <Tag size={14} /> },
            { id: "environment", label: "Environment", icon: <Wrench size={14} /> },
            { id: "workspace", label: "Workspace", icon: <Warehouse size={14} /> },
            { id: "about", label: "About", icon: <Info size={14} /> },
          ]}
        />
      }
    >
      {tab === "github" && (
        <GitHubTab settings={settings} setSettings={setSettings} account={account} accountError={accountError} setAccount={setAccount} />
      )}
      {tab === "categories" && <CategoriesTab ws={ws} projects={projects} onConfig={onConfig} />}
      {tab === "environment" && <EnvironmentTab env={env} />}
      {tab === "workspace" && <WorkspaceTab ws={ws} projects={projects} onRemove={onRemove} />}
      {tab === "about" && <AboutTab />}
    </Modal>
  );
}
