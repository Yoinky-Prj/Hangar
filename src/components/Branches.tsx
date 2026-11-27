import { useCallback, useEffect, useState, type ReactNode } from "react";
import { ArrowDownToLine, ChevronDown, ChevronRight, CircleDot, Cloud, GitBranch, GitBranchPlus, LoaderCircle, Trash } from "lucide-react";
import { api, errText, type BranchList, type GitStatus, type LocalBranch, type RemoteBranch } from "../api";
import { startJob } from "../jobs";
import { baseName, relTime } from "../util";
import { useToast } from "./common";

/**
 * Local and remote branches of one repo, with switch, check out, create,
 * fast-forward and (safe) delete. `owner` is the project whose status to
 * reload afterwards (the repo itself, or the parent of a submodule).
 */
export function BranchesSection({
  path,
  owner,
  status,
  reload,
}: {
  path: string;
  owner: string;
  status: GitStatus | null | undefined;
  reload: (owner: string) => void;
}) {
  const toast = useToast();
  const [list, setList] = useState<BranchList | null | undefined>(undefined);
  const [busy, setBusy] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [newName, setNewName] = useState("");
  const [switchTo, setSwitchTo] = useState(true);
  const [confirmDelete, setConfirmDelete] = useState<string | null>(null);
  const [showRemote, setShowRemote] = useState(false);

  const load = useCallback(() => {
    api.gitBranches(path).then(setList).catch(() => setList(null));
  }, [path]);

  // Reload when the repo changes (fetch, pull, switch, commits made elsewhere).
  const statusKey = status
    ? [status.branch, status.lastCommit?.id, status.ahead, status.behind, status.detached].join("|")
    : "";
  useEffect(() => {
    load();
  }, [load, statusKey]);

  async function run(key: string, fn: () => Promise<string>) {
    if (busy) return;
    setBusy(key);
    const end = startJob(`Changing branches in ${baseName(path)}`);
    try {
      toast("ok", await fn());
    } catch (e) {
      toast("error", errText(e));
    } finally {
      end();
      setBusy(null);
      setConfirmDelete(null);
      reload(owner);
      load();
    }
  }

  const subNote = status && status.submodules > 0 ? " Submodules weren't changed; check whether this branch pins different commits." : "";

  const switchTo_ = (b: LocalBranch) =>
    run(`switch:${b.name}`, async () => {
      await api.gitSwitchBranch(path, b.name);
      return `Switched to ${b.name}.${subNote}`;
    });
  const update = (b: LocalBranch) =>
    run(`update:${b.name}`, async () => {
      const out = await api.gitUpdateBranch(path, b.name);
      return out.commits ? `Fast-forwarded ${b.name} by ${out.commits} commit${out.commits === 1 ? "" : "s"}.` : `${b.name} is already up to date.`;
    });
  const remove = (b: LocalBranch) =>
    run(`delete:${b.name}`, async () => {
      await api.gitDeleteBranch(path, b.name);
      return `Deleted ${b.name}.`;
    });
  const checkout = (rb: RemoteBranch) =>
    run(`checkout:${rb.name}`, async () => {
      const name = await api.gitCheckoutRemote(path, rb.name);
      return `Checked out ${rb.name} as ${name}.${subNote}`;
    });
  const create = () =>
    run("create", async () => {
      const name = await api.gitCreateBranch(path, newName, null, switchTo);
      setNewName("");
      setCreating(false);
      return switchTo ? `Created ${name} and switched to it.` : `Created ${name}.`;
    });

  const spin = (key: string, icon: ReactNode) => (busy === key ? <LoaderCircle size={13} className="spin" /> : icon);

  if (!status || status.unborn) return null;
  const remoteOnly = list?.remote.filter((r) => !r.local) ?? [];

  return (
    <div className="dsec">
      <div className="dsec-head">
        <h3>Branches</h3>
        <button className="btn small" onClick={() => setCreating((c) => !c)} title="Create a branch from the current commit">
          <GitBranchPlus size={13} />
          New branch
        </button>
      </div>

      {creating && (
        <div className="branch-new">
          <input
            className="input mono"
            placeholder="feature/my-change"
            value={newName}
            autoFocus
            onChange={(e) => setNewName(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && newName.trim()) create();
              if (e.key === "Escape") setCreating(false);
            }}
          />
          <div className="row" style={{ marginTop: 8 }}>
            <label className="check" style={{ margin: 0 }}>
              <input type="checkbox" checked={switchTo} onChange={(e) => setSwitchTo(e.target.checked)} />
              Switch to it
            </label>
            <span className="hint">from {status.branch ?? `commit ${status.lastCommit?.id ?? ""}`}</span>
            <span className="spacer" />
            <button className="btn small" onClick={() => setCreating(false)}>
              Cancel
            </button>
            <button className="btn small primary" disabled={!newName.trim() || !!busy} onClick={create}>
              {spin("create", null)}
              Create
            </button>
          </div>
        </div>
      )}

      {list === undefined ? (
        <div className="hint row">
          <LoaderCircle size={14} className="spin" /> Reading branches…
        </div>
      ) : list === null ? (
        <div className="hint">Couldn't read the branches.</div>
      ) : (
        <div className="branch-list">
          {list.local.map((b) => (
            <div key={b.name} className={`branch-row ${b.current ? "current" : ""}`}>
              {b.current ? <CircleDot size={14} className="branch-icon current" /> : <GitBranch size={14} className="branch-icon" />}
              <div className="branch-main">
                <div className="branch-name">
                  <span className="mono" title={b.name}>
                    {b.name}
                  </span>
                  {b.name === list.defaultBranch && <span className="pill">default</span>}
                </div>
                <div className="branch-sub">
                  {b.upstream
                    ? b.ahead || b.behind
                      ? `${b.upstream} · ↑${b.ahead ?? 0} ↓${b.behind ?? 0}`
                      : `${b.upstream} · up to date`
                    : b.upstreamGone
                      ? "upstream was deleted on the remote"
                      : "local only"}
                  {b.lastCommit?.time && ` · ${relTime(b.lastCommit.time)}`}
                </div>
              </div>
              <div className="branch-actions">
                {b.upstream && (b.behind ?? 0) > 0 && (b.ahead ?? 0) === 0 && (
                  <button
                    className="btn small"
                    disabled={!!busy}
                    title={b.current ? "Pull (fast-forward only)" : `Fast-forward ${b.name} to ${b.upstream} without switching to it`}
                    onClick={() => update(b)}
                  >
                    {spin(`update:${b.name}`, <ArrowDownToLine size={13} />)}
                    {b.behind}
                  </button>
                )}
                {!b.current && (
                  <button className="btn small" disabled={!!busy} title="Switch to this branch" onClick={() => switchTo_(b)}>
                    {spin(`switch:${b.name}`, null)}
                    Switch
                  </button>
                )}
                {!b.current &&
                  (confirmDelete === b.name ? (
                    <>
                      <button className="btn small danger" disabled={!!busy} onClick={() => remove(b)}>
                        {spin(`delete:${b.name}`, null)}
                        Delete
                      </button>
                      <button className="btn small" onClick={() => setConfirmDelete(null)}>
                        Keep
                      </button>
                    </>
                  ) : (
                    <button
                      className="btn small ghost icon"
                      disabled={!b.safeToDelete || !!busy}
                      title={b.safeToDelete ? "Delete this branch" : "Has commits that aren't pushed or merged, so it can't be deleted here"}
                      onClick={() => setConfirmDelete(b.name)}
                    >
                      <Trash size={13} />
                    </button>
                  ))}
              </div>
            </div>
          ))}
        </div>
      )}

      {remoteOnly.length > 0 && (
        <>
          <button className="link-toggle" onClick={() => setShowRemote((v) => !v)}>
            {showRemote ? <ChevronDown size={13} /> : <ChevronRight size={13} />}
            {showRemote ? "Hide" : "Show"} {remoteOnly.length} remote branch{remoteOnly.length === 1 ? "" : "es"}
          </button>
          {showRemote && (
            <div className="branch-list" style={{ marginTop: 6 }}>
              {remoteOnly.map((rb) => (
                <div key={rb.name} className="branch-row">
                  <Cloud size={14} className="branch-icon" />
                  <div className="branch-main">
                    <div className="branch-name">
                      <span className="mono" title={rb.name}>
                        {rb.name}
                      </span>
                    </div>
                    <div className="branch-sub" title={rb.lastCommit?.summary ?? ""}>
                      {rb.lastCommit ? `${rb.lastCommit.summary} · ${relTime(rb.lastCommit.time)}` : ""}
                    </div>
                  </div>
                  <div className="branch-actions">
                    <button
                      className="btn small"
                      disabled={!!busy}
                      title={`Create a local ${rb.branch} that tracks ${rb.name}, and switch to it`}
                      onClick={() => checkout(rb)}
                    >
                      {spin(`checkout:${rb.name}`, null)}
                      Check out
                    </button>
                  </div>
                </div>
              ))}
            </div>
          )}
        </>
      )}
      <div className="hint" style={{ marginTop: 6 }}>
        Branch counts are as of the last fetch.
      </div>
    </div>
  );
}
