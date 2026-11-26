import { useCallback, useEffect, useState } from "react";
import { api, errText, type Environment, type Settings, type WorkspaceView } from "./api";
import { CloseGuard } from "./components/CloseGuard";
import { ToastProvider, useToast } from "./components/common";
import { EnvBlocker, Splash, StartError, WorkspacePicker } from "./components/StartScreens";
import { TooltipLayer, useNoNativeContextMenu } from "./components/Tooltips";
import { WorkspaceScreen } from "./components/WorkspaceScreen";

function Root() {
  const toast = useToast();
  const [booting, setBooting] = useState(true);
  const [bootError, setBootError] = useState<string | null>(null);
  const [env, setEnv] = useState<Environment | null>(null);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [ws, setWs] = useState<WorkspaceView | null>(null);

  const boot = useCallback(async () => {
    setBooting(true);
    try {
      const [e, s, intent] = await Promise.all([api.environment(), api.getSettings(), api.launchIntent()]);
      setEnv(e);
      setSettings(s);
      // Jump list: a workspace, or "New window" (the start screen). A plain
      // start reopens the last workspace.
      const asked = intent.workspace;
      const target = asked ?? (intent.startScreen ? null : s.lastWorkspace);
      if (e.ready && target) {
        const insp = await api.inspectFolder(target);
        if (insp.isWorkspace) {
          setWs(await api.openWorkspace(target));
          setSettings(await api.getSettings());
        } else if (asked) {
          toast("error", insp.exists ? `${target} isn't a Hangar workspace.` : `${target} isn't there any more.`);
        }
      }
    } catch (err) {
      setBootError(errText(err));
    } finally {
      setBooting(false);
    }
    // Runs once per window (toast is only used for a failed jump-list target).
  }, []);

  useEffect(() => {
    boot();
  }, [boot]);

  // The window title names the workspace (taskbar thumbnails, Alt+Tab), and
  // the app knows which window to bring forward for a jump-list click.
  useEffect(() => {
    api.setWindowWorkspace(ws?.root ?? null, ws?.config.name ?? null).catch(() => {});
  }, [ws?.root, ws?.config.name]);

  if (booting) return <Splash />;
  if (bootError || !env || !settings) return <StartError message={bootError ?? "unknown error"} />;
  if (!env.ready) return <EnvBlocker env={env} onRetry={boot} />;
  if (!ws)
    return (
      <WorkspacePicker
        settings={settings}
        setSettings={setSettings}
        onOpened={async (w) => {
          setWs(w);
          setSettings(await api.getSettings());
        }}
      />
    );
  return (
    <WorkspaceScreen
      key={ws.root}
      ws={ws}
      setWs={setWs}
      env={env}
      settings={settings}
      setSettings={setSettings}
      onLeave={() => setWs(null)}
    />
  );
}

export default function App() {
  useNoNativeContextMenu();
  return (
    <ToastProvider>
      <Root />
      <CloseGuard />
      <TooltipLayer />
    </ToastProvider>
  );
}
