import { useEffect, useState, type CSSProperties, type ReactNode } from "react";
import { api, type AppIcons, type Environment } from "../api";

/* ---------- app icons ---------- */

let iconsPromise: Promise<AppIcons> | null = null;

/** Icons read from installed apps (currently VS Code's own logo). Fetched once. */
export function useAppIcons(): AppIcons {
  const [icons, setIcons] = useState<AppIcons>({ vscode: null });
  useEffect(() => {
    iconsPromise ??= api.appIcons().catch(() => ({ vscode: null }));
    let alive = true;
    iconsPromise.then((i) => alive && setIcons(i));
    return () => {
      alive = false;
    };
  }, []);
  return icons;
}

export function toolFound(env: Environment, name: string): boolean {
  return !!env.tools.find((t) => t.name === name)?.found;
}

export function ExplorerIcon({ size = 20 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" aria-hidden>
      <path d="M2.5 6.5A2.5 2.5 0 0 1 5 4h4.2c.6 0 1.2.25 1.65.68L12.3 6H19a2.5 2.5 0 0 1 2.5 2.5V10h-19z" fill="#d99a1e" />
      <path d="M2.5 9.5h19v8A2.5 2.5 0 0 1 19 20H5a2.5 2.5 0 0 1-2.5-2.5z" fill="#ffc83d" />
      <path d="M2.5 9.5h19v1.2h-19z" fill="#ffe08a" opacity="0.7" />
    </svg>
  );
}

export function TerminalIcon({ size = 20 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" aria-hidden>
      <rect x="2" y="3.5" width="20" height="17" rx="3.5" fill="#1f2228" stroke="#4a515e" strokeWidth="1" />
      <path d="M6.5 9.5l3.2 2.6-3.2 2.6" fill="none" stroke="#4cc2ff" strokeWidth="1.9" strokeLinecap="round" strokeLinejoin="round" />
      <path d="M11.8 15.4h5.2" stroke="#e8ecf2" strokeWidth="1.9" strokeLinecap="round" />
    </svg>
  );
}

/** GitHub's mark (Octicons `mark-github`). */
export function GitHubMark({ size = 18 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 16 16" fill="currentColor" aria-hidden>
      <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0016 8c0-4.42-3.58-8-8-8z" />
    </svg>
  );
}

/** The installed VS Code's logo, or a stand-in in VS Code blue. */
export function VsCodeIcon({ size = 20, src }: { size?: number; src: string | null }) {
  if (src) return <img src={src} width={size} height={size} alt="" draggable={false} />;
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" aria-hidden fill="none" stroke="#2c9cf0" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M17 3.5v17L7 12.5l-3.5 2.8V8.7L7 11.5z" />
    </svg>
  );
}

/* ---------- launcher tiles ---------- */

function Launcher({
  brand,
  icon,
  title,
  sub,
  disabled,
  hint,
  onClick,
}: {
  brand: string;
  icon: ReactNode;
  title: string;
  sub: string;
  disabled?: boolean;
  hint?: string;
  onClick: () => void;
}) {
  return (
    <button className="launcher" style={{ "--brand": brand } as CSSProperties} disabled={disabled} title={hint} onClick={onClick}>
      <span className="launcher-icon">{icon}</span>
      <span className="launcher-text">
        <span className="launcher-title">{title}</span>
        <span className="launcher-sub">{sub}</span>
      </span>
    </button>
  );
}

export type OpenKind = "explorer" | "vscode" | "terminal";

export function OpenButtons({
  path,
  remote,
  env,
  onOpen,
  onOpenUrl,
}: {
  path: string;
  remote: { host: string; url: string } | null;
  env: Environment;
  onOpen: (kind: OpenKind, path: string) => void;
  onOpenUrl: (url: string) => void;
}) {
  const icons = useAppIcons();
  const hasCode = toolFound(env, "VS Code");
  const hasTerm = toolFound(env, "Terminal");
  return (
    <div className="launchers">
      <Launcher brand="#ffc83d" icon={<ExplorerIcon />} title="Explorer" sub="Show the folder" onClick={() => onOpen("explorer", path)} />
      <Launcher
        brand="#2c9cf0"
        icon={<VsCodeIcon src={icons.vscode} />}
        title="VS Code"
        sub="Open the folder"
        disabled={!hasCode}
        hint={hasCode ? undefined : "VS Code wasn't found. See Settings → Environment."}
        onClick={() => onOpen("vscode", path)}
      />
      <Launcher
        brand="#4cc2ff"
        icon={<TerminalIcon />}
        title="Terminal"
        sub="Open here"
        disabled={!hasTerm}
        hint={hasTerm ? undefined : "No terminal found. See Settings → Environment."}
        onClick={() => onOpen("terminal", path)}
      />
      {remote && (
        <Launcher
          brand="var(--text)"
          icon={<GitHubMark />}
          title={remote.host === "github.com" ? "GitHub" : remote.host}
          sub="Open repo page"
          onClick={() => onOpenUrl(remote.url)}
        />
      )}
    </div>
  );
}
