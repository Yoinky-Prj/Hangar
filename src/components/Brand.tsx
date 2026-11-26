import appIcon from "../../src-tauri/icons/128x128@2x.png";

/** The hangar from the app icon, as a one-color glyph (the door is a cut-out). */
export function HangarMark({ size = 16 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="currentColor" aria-hidden>
      <path
        fillRule="evenodd"
        d="M5.34 17.75V14.06A6.66 6.66 0 0 1 18.66 14.06V17.75Z M7.88 17.75V15.09a.7.7 0 0 1 .7-.7h6.84a.7.7 0 0 1 .7.7v2.66Z"
      />
      <rect x="4.64" y="17.62" width="14.72" height="0.95" rx="0.47" />
    </svg>
  );
}

/** The full-color app icon. */
export function AppIcon({ size = 64, className }: { size?: number; className?: string }) {
  return <img src={appIcon} width={size} height={size} alt="" draggable={false} className={className} />;
}
