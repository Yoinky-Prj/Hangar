import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Info, LoaderCircle } from "lucide-react";
import { onJobsChange, runningJobs, whenIdle, type RunningJob } from "../jobs";
import { Modal } from "./common";

/**
 * Catches every way of closing the window (× button, Alt+F4, taskbar) and,
 * while something is still running, asks first instead of cutting it off.
 */
export function CloseGuard() {
  const [asking, setAsking] = useState(false);
  const [jobs, setJobs] = useState<RunningJob[]>([]);
  const [stopping, setStopping] = useState(false);

  useEffect(() => {
    const win = getCurrentWindow();
    let unlisten: (() => void) | undefined;
    let alive = true;
    win
      .onCloseRequested((e) => {
        if (runningJobs().length === 0) return; // nothing running: close normally
        e.preventDefault();
        setJobs(runningJobs());
        setAsking(true);
      })
      .then((f) => (alive ? (unlisten = f) : f()));
    return () => {
      alive = false;
      unlisten?.();
    };
  }, []);

  // Keep the list current while the question is open.
  useEffect(() => {
    if (!asking) return;
    return onJobsChange(() => setJobs(runningJobs()));
  }, [asking]);

  const close = () => getCurrentWindow().destroy();

  async function cancelAndClose() {
    setStopping(true);
    await Promise.allSettled(runningJobs().map((j) => j.cancel?.()));
    // Give cancelled jobs time to clean up (e.g. remove a half-cloned folder).
    await whenIdle(15000);
    close();
  }

  if (!asking) return null;
  const cancellable = jobs.some((j) => j.cancel);
  return (
    <Modal title="Still working" onClose={() => !stopping && setAsking(false)} width={480} locked={stopping} zIndex={90}>
      <div className="callout warn" style={{ marginBottom: 12 }}>
        <Info size={15} />
        <div>
          Closing Hangar now stops this partway.
          {jobs.some((j) => j.label.startsWith("Cloning") || j.label.startsWith("Importing")) &&
            " A half-copied folder would be left in the workspace."}
        </div>
      </div>
      {jobs.length === 0 ? (
        <div className="hint">Everything has finished. You can close Hangar now.</div>
      ) : (
        <ul className="job-list">
          {jobs.map((j) => (
            <li key={j.id}>
              <LoaderCircle size={13} className="spin" />
              {j.label}
            </li>
          ))}
        </ul>
      )}
      <div className="row" style={{ marginTop: 16, justifyContent: "flex-end", flexWrap: "wrap" }}>
        {jobs.length === 0 ? (
          <button className="btn primary" onClick={close}>
            Close Hangar
          </button>
        ) : (
          <>
            <button className="btn danger" onClick={close} disabled={stopping} title="Close right away, leaving the work unfinished">
              Close anyway
            </button>
            {cancellable && (
              <button className="btn" onClick={cancelAndClose} disabled={stopping} title="Stop it cleanly (removing any half-cloned folder), then close">
                {stopping && <LoaderCircle size={13} className="spin" />}
                {stopping ? "Stopping…" : "Cancel it and close"}
              </button>
            )}
            <button className="btn primary" onClick={() => setAsking(false)} disabled={stopping} autoFocus>
              Keep working
            </button>
          </>
        )}
      </div>
    </Modal>
  );
}
