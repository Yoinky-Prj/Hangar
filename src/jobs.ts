/**
 * Long-running work in progress (clone, import, fetch, pull, branch switch),
 * so closing the window can warn instead of cutting it off halfway.
 */

export interface RunningJob {
  id: string;
  label: string;
  /** Asks the job to stop; the backend then cleans up (e.g. removes a half-cloned folder). */
  cancel?: () => Promise<unknown>;
}

const jobs = new Map<string, RunningJob>();
const listeners = new Set<() => void>();

function changed() {
  for (const l of listeners) l();
}

/** Register a job; call the returned function when it ends (success or not). */
export function startJob(label: string, cancel?: () => Promise<unknown>): () => void {
  const id = crypto.randomUUID();
  jobs.set(id, { id, label, cancel });
  changed();
  return () => {
    if (jobs.delete(id)) changed();
  };
}

export function runningJobs(): RunningJob[] {
  return [...jobs.values()];
}

export function onJobsChange(fn: () => void): () => void {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

/** Resolves once nothing is running (or after `timeoutMs`). */
export function whenIdle(timeoutMs: number): Promise<void> {
  return new Promise((resolve) => {
    if (jobs.size === 0) return resolve();
    const timer = window.setTimeout(done, timeoutMs);
    const off = onJobsChange(() => jobs.size === 0 && done());
    function done() {
      window.clearTimeout(timer);
      off();
      resolve();
    }
  });
}
