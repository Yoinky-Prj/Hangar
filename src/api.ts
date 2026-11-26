import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type Section = "projects" | "reference" | "archive";
export type SourceKind = "clone" | "new" | "import" | "adopted";

export interface RemoteInfo {
  host: string;
  owner: string;
  repo: string;
  url: string;
}

export interface ProjectMeta {
  version: number;
  id: string;
  /** Display name; unset = show owner/repo (or the folder name). */
  name?: string;
  section: Section;
  archivedFrom?: Section;
  archivedAt?: string;
  categories: string[];
  source: { kind: SourceKind; remote?: RemoteInfo; importedFrom?: string };
  createdAt: string;
}

export interface ProjectEntry {
  folder: string;
  path: string;
  section: Section;
  managed: boolean;
  isGit: boolean;
  meta: ProjectMeta | null;
  modified: string | null;
}

export interface Category {
  id: string;
  name: string;
  color: string;
}

export interface WorkspaceConfig {
  version: number;
  name: string;
  createdAt: string;
  categories: Category[];
}

export interface WorkspaceView {
  root: string;
  config: WorkspaceConfig;
}

export interface FolderInspection {
  exists: boolean;
  isWorkspace: boolean;
  entryCount: number;
  name: string;
}

/** Neither set: a plain start, which reopens the last workspace. */
export interface LaunchIntent {
  workspace: string | null;
  /** "New window": always the start screen. */
  startScreen: boolean;
}

export interface UpdateInfo {
  version: string;
  /** The release's notes (Markdown as written on GitHub). */
  notes: string | null;
  date: string | null;
}

export interface UpdateStatus {
  current: string;
  /** Installed with the installer, so it can update itself; the standalone exe can't. */
  installed: boolean;
  available: UpdateInfo | null;
}

/** One entry of the workspace list. */
export interface WorkspaceSummary {
  path: string;
  /** The workspace's own name, or the folder name if it isn't set up. */
  name: string;
  exists: boolean;
  isWorkspace: boolean;
  entryCount: number;
  /** Project folders in Projects, Reference and .archive. */
  projects: number;
}

export interface CommitInfo {
  id: string;
  summary: string;
  author: string;
  time: string | null;
}

export interface GitStatus {
  branch: string | null;
  detached: boolean;
  unborn: boolean;
  staged: number;
  modified: number;
  untracked: number;
  conflicted: number;
  upstream: string | null;
  ahead: number | null;
  behind: number | null;
  unpushed: number;
  unpushedCapped: boolean;
  remotes: { name: string; url: string }[];
  lastCommit: CommitInfo | null;
  submodules: number;
  worktrees: number;
  /** The remote's default branch, e.g. "main". */
  defaultBranch: string | null;
  /** Cloned with "latest commit only": history is cut off. */
  shallow: boolean;
  /** origin only fetches one branch. */
  singleBranch: boolean;
}

export interface LocalBranch {
  name: string;
  current: boolean;
  upstream: string | null;
  upstreamGone: boolean;
  ahead: number | null;
  behind: number | null;
  lastCommit: CommitInfo | null;
  /** Deleting loses nothing: every commit is on a remote or in the current branch. */
  safeToDelete: boolean;
}

export interface RemoteBranch {
  /** e.g. "origin/feat/x" */
  name: string;
  remote: string;
  branch: string;
  lastCommit: CommitInfo | null;
  /** Local branch tracking it, if any. */
  local: string | null;
}

export interface BranchList {
  local: LocalBranch[];
  remote: RemoteBranch[];
  defaultBranch: string | null;
  detached: boolean;
}

export interface RepoRef {
  host: string;
  owner: string;
  repo: string;
}

export interface SubmoduleInfo {
  name: string;
  /** Path inside the parent repo, e.g. "apps/api". */
  path: string;
  absPath: string;
  url: string | null;
  remote: RepoRef | null;
  initialized: boolean;
  /** Commit the parent records, and the one checked out. */
  recorded: string | null;
  checkedOut: string | null;
  moved: boolean;
  status: GitStatus | null;
}

export interface AppIcons {
  vscode: string | null;
}

export interface FetchOutcome {
  remotes: string[];
  receivedObjects: number;
}

export interface PullOutcome {
  branch: string;
  upstream: string;
  /** Commits fast-forwarded; 0 = already up to date. */
  commits: number;
  movedSubmodules: string[];
}

export interface AttachOutcome {
  path: string;
  /** Branch it's on now; null = still detached (see note). */
  branch: string | null;
  note: string | null;
}

export interface Settings {
  /** Every workspace added, in the order they were added. */
  workspaces: string[];
  lastWorkspace: string | null;
  githubClientId: string | null;
}

export interface Tool {
  name: string;
  found: boolean;
  path: string | null;
  required: boolean;
  note: string;
}

export interface Environment {
  tools: Tool[];
  ready: boolean;
}

export interface TokenInfo {
  /** "device" = signed in through github.com, "pat" = personal access token. */
  kind: "device" | "pat";
  /** When the current access token expires; null = it doesn't. */
  expiresAt: string | null;
  renews: boolean;
  renewUntil: string | null;
  patExpires: string | null;
}

export interface Account {
  login: string;
  name: string | null;
  avatarUrl: string | null;
  orgs: string[];
  scopes: string | null;
  token: TokenInfo;
}

export interface GhRepo {
  fullName: string;
  owner: string;
  name: string;
  private: boolean;
  fork: boolean;
  archived: boolean;
  description: string | null;
  pushedAt: string | null;
  htmlUrl: string;
  stars: number;
  language: string | null;
}

export interface DeviceCode {
  deviceCode: string;
  userCode: string;
  verificationUri: string;
  expiresIn: number;
  interval: number;
}

export type PollResult =
  | { status: "pending" }
  | { status: "slowDown"; interval: number }
  | { status: "done"; account: Account }
  | { status: "expired" }
  | { status: "denied" };

export interface ImportInspection {
  path: string;
  folder: string;
  isGit: boolean;
  remote: RepoRef | null;
  suggestedName: string;
}

export interface ImportReport {
  files: number;
  bytes: number;
  skippedDirs: string[];
  skippedLinks: string[];
}

export interface JobProgress {
  jobId: string;
  stage: string;
  current: number;
  total: number;
  bytes: number;
  detail: string | null;
}

export interface CloneArgs {
  input: string;
  section: Section;
  categories: string[];
  recurseSubmodules: boolean;
  folder?: string;
  displayName?: string;
  /** Branch to check out; unset = the remote's default. */
  branch?: string;
  /** Only the latest commit of that one branch. */
  shallow?: boolean;
}

export interface ImportArgs {
  source: string;
  section: Section;
  /** Folder name in the workspace. */
  name: string;
  displayName?: string;
  categories: string[];
  skipRegenerable: boolean;
}

export const api = {
  getSettings: () => invoke<Settings>("get_settings"),
  setGithubClientId: (clientId: string) => invoke<Settings>("set_github_client_id", { clientId }),
  forgetWorkspace: (path: string) => invoke<Settings>("forget_workspace", { path }),
  listWorkspaces: () => invoke<WorkspaceSummary[]>("list_workspaces"),
  /** What this window was opened for (jump list, second launch). */
  launchIntent: () => invoke<LaunchIntent>("launch_intent"),
  /** Tell the app which workspace this window shows (null: the start screen). */
  setWindowWorkspace: (path: string | null, name: string | null) => invoke<void>("set_window_workspace", { path, name }),
  environment: () => invoke<Environment>("environment"),
  updateStatus: () => invoke<UpdateStatus>("update_status"),
  checkForUpdate: () => invoke<UpdateInfo | null>("check_for_update"),
  /** Downloads and runs the installer; Hangar exits and the installer starts it again. */
  installUpdate: () => invoke<void>("install_update"),

  inspectFolder: (path: string) => invoke<FolderInspection>("inspect_folder", { path }),
  initWorkspace: (path: string) => invoke<WorkspaceView>("init_workspace", { path }),
  openWorkspace: (path: string) => invoke<WorkspaceView>("open_workspace", { path }),
  scanProjects: (root: string) => invoke<ProjectEntry[]>("scan_projects", { root }),
  gitStatus: (path: string) => invoke<GitStatus>("git_status", { path }),
  gitSubmodules: (path: string) => invoke<SubmoduleInfo[]>("git_submodules", { path }),
  attachSubmodules: (path: string, only?: string) => invoke<AttachOutcome[]>("attach_submodules", { path, only: only ?? null }),
  gitFetch: (path: string) => invoke<FetchOutcome>("git_fetch", { path }),
  gitPull: (path: string) => invoke<PullOutcome>("git_pull", { path }),
  gitFullHistory: (path: string) => invoke<void>("git_full_history", { path }),
  gitBranches: (path: string) => invoke<BranchList>("git_branches", { path }),
  gitSwitchBranch: (path: string, name: string) => invoke<void>("git_switch_branch", { path, name }),
  gitCheckoutRemote: (path: string, remoteBranch: string) => invoke<string>("git_checkout_remote", { path, remoteBranch }),
  gitCreateBranch: (path: string, name: string, from: string | null, switchTo: boolean) =>
    invoke<string>("git_create_branch", { path, name, from, switch: switchTo }),
  gitUpdateBranch: (path: string, name: string) => invoke<PullOutcome>("git_update_branch", { path, name }),
  gitDeleteBranch: (path: string, name: string) => invoke<void>("git_delete_branch", { path, name }),
  githubBranches: (owner: string, repo: string) => invoke<string[]>("github_branches", { owner, repo }),
  githubSearch: (query: string) => invoke<GhRepo[]>("github_search", { query }),
  appIcons: () => invoke<AppIcons>("app_icons"),

  addCategory: (root: string, name: string, color: string) =>
    invoke<WorkspaceConfig>("add_category", { root, name, color }),
  updateCategory: (root: string, id: string, name: string, color: string) =>
    invoke<WorkspaceConfig>("update_category", { root, id, name, color }),
  deleteCategory: (root: string, id: string) => invoke<WorkspaceConfig>("delete_category", { root, id }),
  setProjectCategories: (root: string, path: string, categories: string[]) =>
    invoke<ProjectEntry>("set_project_categories", { root, path, categories }),
  setProjectName: (root: string, path: string, name: string | null) =>
    invoke<ProjectEntry>("set_project_name", { root, path, name }),

  moveProject: (root: string, path: string, target: Section) =>
    invoke<ProjectEntry>("move_project", { root, path, target }),
  unarchiveProject: (root: string, path: string) => invoke<ProjectEntry>("unarchive_project", { root, path }),
  adoptProject: (root: string, path: string) => invoke<ProjectEntry>("adopt_project", { root, path }),

  suggestFolderName: (name: string) => invoke<string>("suggest_folder_name", { name }),
  createProject: (root: string, args: { name: string; section: Section; initGit: boolean; categories: string[] }) =>
    invoke<ProjectEntry>("create_project", { root, args }),

  parseRepo: (input: string) => invoke<RepoRef | null>("parse_repo", { input }),
  cloneRepo: (root: string, jobId: string, args: CloneArgs) => invoke<ProjectEntry>("clone_repo", { root, jobId, args }),
  cancelJob: (jobId: string) => invoke<void>("cancel_job", { jobId }),

  inspectImport: (path: string) => invoke<ImportInspection>("inspect_import", { path }),
  importProject: (root: string, jobId: string, args: ImportArgs) =>
    invoke<{ project: ProjectEntry; report: ImportReport }>("import_project", { root, jobId, args }),

  githubAccount: () => invoke<Account | null>("github_account"),
  githubStartDevice: () => invoke<DeviceCode>("github_start_device"),
  githubPollDevice: (deviceCode: string) => invoke<PollResult>("github_poll_device", { deviceCode }),
  githubSignInToken: (token: string) => invoke<Account>("github_sign_in_token", { token }),
  githubSignOut: () => invoke<void>("github_sign_out"),
  githubListRepos: () => invoke<GhRepo[]>("github_list_repos"),

  openInExplorer: (path: string) => invoke<void>("open_in_explorer", { path }),
  openInVscode: (path: string) => invoke<void>("open_in_vscode", { path }),
  openInTerminal: (path: string) => invoke<void>("open_in_terminal", { path }),
  openUrl: (url: string) => invoke<void>("open_url", { url }),
};

export function onJobProgress(cb: (p: JobProgress) => void): Promise<UnlistenFn> {
  return listen<JobProgress>("job-progress", (e) => cb(e.payload));
}

export function onUpdateAvailable(cb: (u: UpdateInfo) => void): Promise<UnlistenFn> {
  return listen<UpdateInfo>("update-available", (e) => cb(e.payload));
}

/** Bytes of the installer downloaded so far, and its size when known. */
export function onUpdateProgress(cb: (done: number, total: number | null) => void): Promise<UnlistenFn> {
  return listen<[number, number | null]>("update-progress", (e) => cb(e.payload[0], e.payload[1]));
}

export function errText(e: unknown): string {
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  return JSON.stringify(e);
}

export const SECTION_LABEL: Record<Section, string> = {
  projects: "Projects",
  reference: "Reference",
  archive: "Archive",
};

export function remoteKey(r: { host: string; owner: string; repo: string }): string {
  return `${r.host}/${r.owner}/${r.repo}`.toLowerCase();
}
