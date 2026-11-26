import { useState, type ReactNode } from "react";
import { Archive, BookOpen, Briefcase, FolderOpen, Info, LoaderCircle, Settings2 } from "lucide-react";
import { api, errText, type WorkspaceView } from "../api";
import { baseName } from "../util";
import { Modal, useToast } from "./common";

const LAYOUT: { name: string; icon: ReactNode; tone: string; text: string }[] = [
  { name: ".hangar", icon: <Settings2 size={15} />, tone: "slate", text: "Hangar's own data: categories and a project index" },
  { name: "Projects", icon: <Briefcase size={15} />, tone: "blue", text: "Your own work" },
  { name: "Reference", icon: <BookOpen size={15} />, tone: "teal", text: "Code you keep for reference" },
  { name: ".archive", icon: <Archive size={15} />, tone: "amber", text: "Archived projects" },
];

/** What setting up a workspace in `path` will add (shared by the landing page and the dialog). */
export function SetupPreview({ path, entryCount }: { path: string; entryCount: number }) {
  return (
    <>
      <div className="path-chip" title={path}>
        <FolderOpen size={16} />
        <div style={{ minWidth: 0 }}>
          <div className="path-chip-name">{baseName(path)}</div>
          <div className="mono path-chip-path">{path}</div>
        </div>
      </div>
      <div className="panel-label">Hangar will add</div>
      <div className="tree">
        {LAYOUT.map((row) => (
          <div key={row.name} className="tree-row">
            <div className={`tree-icon ${row.tone}`}>{row.icon}</div>
            <div className="tree-name mono">{row.name}</div>
            <div className="tree-text">{row.text}</div>
          </div>
        ))}
      </div>
      {entryCount > 0 && (
        <div className="callout" style={{ marginTop: 14 }}>
          <Info size={15} />
          <div>
            This folder already has {entryCount} item{entryCount === 1 ? "" : "s"}. They stay where they are; Hangar only adds the folders
            above.
          </div>
        </div>
      )}
    </>
  );
}

/** "Set up a new workspace here?" as a dialog, for adding a workspace from the top bar. */
export function WorkspaceSetupDialog({
  path,
  entryCount,
  onCreated,
  onClose,
}: {
  path: string;
  entryCount: number;
  onCreated: (ws: WorkspaceView) => void;
  onClose: () => void;
}) {
  const toast = useToast();
  const [busy, setBusy] = useState(false);

  async function create() {
    setBusy(true);
    try {
      onCreated(await api.initWorkspace(path));
    } catch (e) {
      toast("error", errText(e));
      setBusy(false);
    }
  }

  return (
    <Modal
      title="Set up a new workspace"
      onClose={onClose}
      width={520}
      locked={busy}
      footer={
        <>
          <div className="spacer" />
          <button className="btn" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <button className="btn primary" onClick={create} disabled={busy}>
            {busy && <LoaderCircle size={14} className="spin" />}
            Create workspace
          </button>
        </>
      }
    >
      <SetupPreview path={path} entryCount={entryCount} />
    </Modal>
  );
}
