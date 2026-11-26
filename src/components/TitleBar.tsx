import { useEffect, useState, type ReactNode } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { HangarMark } from "./Brand";
import { UpdateButton } from "./Update";

/** Windows-style minimize / maximize / close buttons for the frameless window. */
export function WindowControls() {
  const [maximized, setMaximized] = useState(false);

  useEffect(() => {
    const win = getCurrentWindow();
    let unlisten: (() => void) | undefined;
    let alive = true;
    const sync = () => win.isMaximized().then((m) => alive && setMaximized(m));
    sync();
    win.onResized(sync).then((f) => {
      if (alive) unlisten = f;
      else f();
    });
    return () => {
      alive = false;
      unlisten?.();
    };
  }, []);

  const win = getCurrentWindow();
  return (
    <div className="winctl">
      <button aria-label="Minimize" title="Minimize" onClick={() => win.minimize()}>
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden>
          <path d="M0 5.5h10" stroke="currentColor" strokeWidth="1" />
        </svg>
      </button>
      <button aria-label={maximized ? "Restore" : "Maximize"} title={maximized ? "Restore" : "Maximize"} onClick={() => win.toggleMaximize()}>
        {maximized ? (
          <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden fill="none" stroke="currentColor" strokeWidth="1">
            <path d="M2.5 2.5v-1a1 1 0 0 1 1-1h5a1 1 0 0 1 1 1v5a1 1 0 0 1-1 1h-1" />
            <rect x="0.5" y="2.5" width="7" height="7" rx="1" />
          </svg>
        ) : (
          <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden fill="none" stroke="currentColor" strokeWidth="1">
            <rect x="0.5" y="0.5" width="9" height="9" rx="1" />
          </svg>
        )}
      </button>
      <button className="close" aria-label="Close" title="Close" onClick={() => win.close()}>
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden stroke="currentColor" strokeWidth="1">
          <path d="M0.5 0.5l9 9M9.5 0.5l-9 9" />
        </svg>
      </button>
    </div>
  );
}

/** Draggable title bar for screens that don't have their own toolbar. */
export function TitleBar({ brand = true, children }: { brand?: boolean; children?: ReactNode }) {
  return (
    <div className="titlebar" data-tauri-drag-region>
      {brand && (
        <div className="titlebar-brand drag-through" data-tauri-drag-region>
          <HangarMark size={16} />
          Hangar
        </div>
      )}
      {children}
      <div className="spacer" data-tauri-drag-region />
      <UpdateButton />
      <WindowControls />
    </div>
  );
}

/** A full-window screen with the title bar on top. */
export function Frame({ brand, className, children }: { brand?: boolean; className?: string; children: ReactNode }) {
  return (
    <div className={`frame ${className ?? ""}`}>
      <TitleBar brand={brand} />
      <div className="frame-body">{children}</div>
    </div>
  );
}
